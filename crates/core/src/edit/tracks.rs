//! Edits once the camera is unlinked: the screen and the camera each have their own clips, kept
//! in step on the timeline by hidden clips (gaps). Removing a clip leaves a gap so the other
//! track stays where it was; the screen track sets the timeline's length.

use super::{
    Clip, Clipboard, Doc, Edge, Edl, Limits, MIN_CLIP_NS, Speed, Track, delete_range, duration,
    index_of, ripple_insert_all, split_at, src_of, starts, tl_of, within,
};
use crate::project::Extra;
use crate::{Error, Result};

pub(super) fn track_of(d: &Doc, id: &str) -> Result<Track> {
    let has = |e: &Edl| e.clips.iter().any(|c| c.id == id);
    if has(&d.edl) {
        Ok(Track::Screen)
    } else if d.camera_edl.as_ref().is_some_and(has) {
        Ok(Track::Camera)
    } else {
        Err(Error::Invalid(format!("no clip {id}")))
    }
}

pub(super) fn edl_mut(d: &mut Doc, track: Track) -> &mut Edl {
    match d.camera_edl {
        Some(ref mut cam) if track == Track::Camera => cam,
        _ => &mut d.edl,
    }
}

/// Removes `[a, b)` from one track, closing it up.
pub(super) fn cut(edl: &mut Edl, a: i64, b: i64) {
    let total = duration(edl);
    if a >= total || b <= a {
        return;
    }
    let i = split_at(edl, a);
    let j = split_at(edl, b.min(total));
    edl.clips.drain(i..j);
}

fn gap(len: i64) -> Clip {
    Clip {
        id: crate::ids::new_id(),
        src_in_rec_ns: 0,
        src_out_rec_ns: len,
        speed: Speed::ONE,
        muted: false,
        hidden: true,
        extra: Extra::new(),
    }
}

struct Placed {
    at: i64,
    clip: Clip,
}

fn placed(edl: &Edl) -> Vec<Placed> {
    let st = starts(edl);
    edl.clips
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.hidden)
        .map(|(i, c)| Placed {
            at: st[i],
            clip: c.clone(),
        })
        .collect()
}

/// The clips back in a list, with gaps between them and, up to `min_len`, after the last.
fn rebuild(mut items: Vec<Placed>, min_len: i64, extra: Extra) -> Edl {
    items.sort_by_key(|p| p.at);
    let mut clips = Vec::with_capacity(items.len() * 2);
    let mut t = 0;
    for p in items {
        if p.at - t > 0 {
            clips.push(gap(p.at - t));
            t = p.at;
        }
        t += p.clip.tl_len();
        clips.push(p.clip);
    }
    if min_len > t {
        clips.push(gap(min_len - t));
    }
    Edl { clips, extra }
}

/// One track's visible clips, where the clip `id` sits among them, and the room around it.
struct Spot {
    track: Track,
    items: Vec<Placed>,
    k: usize,
    prev_end: i64,
    next_start: i64,
}

fn spot(d: &Doc, id: &str) -> Result<Spot> {
    let track = track_of(d, id)?;
    let edl = match track {
        Track::Screen => &d.edl,
        Track::Camera => d.camera_clips(),
    };
    let items = placed(edl);
    let k = items
        .iter()
        .position(|p| p.clip.id == id)
        .ok_or_else(|| Error::Invalid("that is a gap, not a clip".into()))?;
    let prev_end = k
        .checked_sub(1)
        .map_or(0, |j| items[j].at + items[j].clip.tl_len());
    let next_start = items.get(k + 1).map_or(i64::MAX, |p| p.at);
    Ok(Spot {
        track,
        items,
        k,
        prev_end,
        next_start,
    })
}

fn put(d: &mut Doc, track: Track, items: Vec<Placed>) {
    let min_len = match track {
        Track::Screen => d.duration(),
        Track::Camera => 0,
    };
    let edl = edl_mut(d, track);
    *edl = rebuild(items, min_len, edl.extra.clone());
}

pub(super) fn set_linked(d: &mut Doc, linked: bool) -> Result<()> {
    if linked == d.linked() {
        return Ok(());
    }
    if !linked {
        if d.camera_layout.overlays.is_empty() {
            return Err(Error::Invalid("this recording has no camera".into()));
        }
        d.camera_edl = Some(Edl {
            clips: d.edl.clips.iter().map(Clip::fresh).collect(),
            extra: d.edl.extra.clone(),
        });
        return Ok(());
    }
    let st = starts(&d.edl);
    let gaps: Vec<(i64, i64)> = d
        .edl
        .clips
        .iter()
        .enumerate()
        .filter(|(_, c)| c.hidden)
        .map(|(i, _)| (st[i], st[i + 1]))
        .collect();
    for (a, b) in gaps.into_iter().rev() {
        delete_range(d, a, b);
    }
    d.camera_edl = None;
    Ok(())
}

/// A clip becomes a gap; a gap on the camera track closes up; a gap on the screen track takes
/// its time out of the whole timeline.
pub(super) fn delete_clips(d: &mut Doc, ids: &[String]) -> Result<()> {
    let mut screen_gaps = Vec::new();
    for id in ids {
        let track = track_of(d, id)?;
        let edl = edl_mut(d, track);
        let i = index_of(edl, id)?;
        match (edl.clips[i].hidden, track) {
            (false, _) => edl.clips[i].hidden = true,
            (true, Track::Camera) => {
                edl.clips.remove(i);
            }
            (true, Track::Screen) => {
                let st = starts(edl);
                screen_gaps.push((st[i], st[i + 1]));
            }
        }
    }
    screen_gaps.sort_unstable();
    for (a, b) in screen_gaps.into_iter().rev() {
        delete_range(d, a, b);
    }
    Ok(())
}

/// Moves one edge of a clip without moving anything else; it stops at its neighbours.
pub(super) fn trim(d: &mut Doc, id: &str, edge: Edge, rec_ns: i64, lim: Limits) -> Result<()> {
    let Spot {
        track,
        mut items,
        k,
        prev_end,
        next_start,
    } = spot(d, id)?;
    let p = &mut items[k];
    let c = &mut p.clip;
    match edge {
        Edge::Start => {
            let mut new_in = rec_ns.clamp(0, c.src_out_rec_ns - MIN_CLIP_NS);
            let earliest = c.src_in_rec_ns - src_of(p.at - prev_end, c.speed);
            new_in = new_in.max(earliest);
            p.at += tl_of(new_in - c.src_in_rec_ns, c.speed);
            c.src_in_rec_ns = new_in;
        }
        Edge::End => {
            let mut new_out = rec_ns.clamp(c.src_in_rec_ns + MIN_CLIP_NS, lim.rec_len_ns);
            if next_start != i64::MAX {
                new_out = new_out.min(c.src_in_rec_ns + src_of(next_start - p.at, c.speed));
            }
            c.src_out_rec_ns = new_out.max(c.src_in_rec_ns + MIN_CLIP_NS);
        }
    }
    put(d, track, items);
    Ok(())
}

pub(super) fn slide(d: &mut Doc, id: &str, by: i64) -> Result<()> {
    if d.linked() {
        return Err(Error::Invalid(
            "unlink the camera to move a clip in time".into(),
        ));
    }
    let Spot {
        track,
        mut items,
        k,
        prev_end,
        next_start,
    } = spot(d, id)?;
    let len = items[k].clip.tl_len();
    let latest = next_start.saturating_sub(len);
    items[k].at = items[k]
        .at
        .saturating_add(by)
        .clamp(prev_end, latest.max(prev_end));
    put(d, track, items);
    Ok(())
}

pub(super) fn set_speed(d: &mut Doc, ids: &[String], speed: Speed) -> Result<()> {
    if speed.num == 0 || speed.den == 0 {
        return Err(Error::Invalid("speed must be positive".into()));
    }
    for id in ids {
        let Spot {
            track,
            mut items,
            k,
            next_start,
            ..
        } = spot(d, id)?;
        let p = &mut items[k];
        p.clip.speed = speed;
        if p.clip.tl_len() < MIN_CLIP_NS {
            p.clip.src_out_rec_ns = p.clip.src_in_rec_ns + src_of(MIN_CLIP_NS, speed);
        }
        if p.at + p.clip.tl_len() > next_start {
            return Err(Error::Invalid(
                "there's no room after the clip at that speed; leave a gap after it first".into(),
            ));
        }
        put(d, track, items);
    }
    Ok(())
}

/// A copy of each clip right after it, where there is room for one.
pub(super) fn duplicate(d: &mut Doc, ids: &[String]) -> Result<()> {
    for id in ids {
        let Spot {
            track,
            mut items,
            k,
            next_start,
            ..
        } = spot(d, id)?;
        let end = items[k].at + items[k].clip.tl_len();
        if next_start.saturating_sub(end) < items[k].clip.tl_len() {
            return Err(Error::Invalid(
                "there's no room after the clip for a copy".into(),
            ));
        }
        let copy = Placed {
            at: end,
            clip: items[k].clip.fresh(),
        };
        items.push(copy);
        put(d, track, items);
    }
    Ok(())
}

/// Inserts the clips on their own track and a gap as long on the other, so both stay in step.
pub(super) fn paste(d: &mut Doc, at: i64, cb: &Clipboard) {
    if cb.is_empty() {
        return;
    }
    let len = cb.len();
    let on = cb.track.unwrap_or(Track::Screen);
    let at = at.clamp(0, d.duration());
    let i = split_at(&mut d.edl, at);
    let at = starts(&d.edl)[i];
    let fill = |track: Track| -> Vec<Clip> {
        if track == on {
            cb.clips.iter().map(Clip::fresh).collect()
        } else {
            vec![gap(len)]
        }
    };
    for (k, c) in fill(Track::Screen).into_iter().enumerate() {
        d.edl.clips.insert(i + k, c);
    }
    let cam_clips = fill(Track::Camera);
    if let Some(cam) = d.camera_edl.as_mut() {
        let total = duration(cam);
        if at < total {
            let j = split_at(cam, at);
            for (k, c) in cam_clips.into_iter().enumerate() {
                cam.clips.insert(j + k, c);
            }
        } else if on == Track::Camera {
            cam.clips.push(gap(at - total));
            cam.clips.extend(cam_clips);
        }
        cam.clips.retain(|c| c.tl_len() > 0 || !c.hidden);
    }
    ripple_insert_all(d, at, len);
    d.text.extend(within(&cb.text, i64::MIN, i64::MAX, -at));
}
