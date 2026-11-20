//! Screen writer thread: routes frames by stream tag, cuts on the new stream's first frame, and
//! hands closed segments to the finisher thread (ARCHITECTURE §4 "Switching source").

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sc_core::journal::JournalEvent;
use sc_core::project::{
    EndReason, Extra, RegionMethod, ScreenSegment, SourceDescriptor, StartReason, Via,
};
use sc_core::sessionlog::Level;
use sc_geometry::{RectPx, SizePx, pt_to_px};
use sc_platform::{
    Append, FinishedTrack, StreamTag, TrackWriter, VideoFrame, VideoWriterSpec, WriterFactory,
};

use super::shared::{Ctl, Shared, TrackStats, bump, lock};
use crate::testhooks::{HoldPoint, hold};

/// Frames that can reach the router between `start_host` and `Begin` (journal fsync, test hold).
const PRESTART_FRAMES: usize = 16;

pub(crate) enum RouterIn {
    Frame(VideoFrame),
    Cmd(RouterCmd),
}

pub(crate) enum RouterCmd {
    Prepare(Prepared),
    Begin {
        start_host: i64,
    },
    Cancel {
        tag: StreamTag,
        reply: Sender<bool>,
    },
    OldStopped {
        tag: StreamTag,
        reply: Sender<Option<Closed>>,
    },
    Lose {
        tag: StreamTag,
        at_host: i64,
        reply: Sender<Option<Closed>>,
    },
    Stop {
        stop_host: i64,
        reply: Sender<RouterDone>,
    },
}

pub(crate) struct Prepared {
    pub tag: StreamTag,
    pub seg: String,
    pub track_id: String,
    pub file: String,
    /// `None`: the router journals `segment_prepare` and creates the writer at the first frame's
    /// size (s0).
    pub writer: Option<Box<dyn TrackWriter>>,
    pub size: SizePx,
    pub via: Via,
    pub reason: StartReason,
    pub source: SourceDescriptor,
    pub closes_old_with: EndReason,
}

pub(crate) struct Opened {
    pub tag: StreamTag,
    pub segment: ScreenSegment,
    pub old_tag: Option<StreamTag>,
    /// Signalled when the old stream delivers its first frame at or past the cut.
    pub old_passed: Option<Receiver<()>>,
    pub file: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Closed {
    pub seg: String,
    pub end_rec_ns: i64,
    pub reason: EndReason,
}

pub(crate) type Finished = (String, std::result::Result<FinishedTrack, String>);

pub(crate) struct RouterDone {
    pub closed: Vec<Closed>,
    pub finished: Vec<Finished>,
    pub recreated: u32,
}

struct Seg {
    id: String,
    track_id: String,
    tag: StreamTag,
    writer: Box<dyn TrackWriter>,
    start_rec: i64,
    last: Option<VideoFrame>,
    last_file_ns: i64,
    stats: Arc<TrackStats>,
    errors: u32,
}

struct Closing {
    seg: Seg,
    cut_host: i64,
    cut_rec: i64,
    reason: EndReason,
    trailing: u32,
    passed: Option<SyncSender<()>>,
}

struct FinishJob {
    track_id: String,
    writer: Box<dyn TrackWriter>,
    last: Option<VideoFrame>,
    last_file_ns: i64,
    end_file_ns: i64,
    frame_ns: i64,
}

struct Router {
    sh: Arc<Shared>,
    writers: Arc<dyn WriterFactory>,
    spec: VideoWriterSpec,
    frame_ns: i64,
    start_host: Option<i64>,
    /// Frames of s0's stream that arrived before `Begin` (newest last, bounded).
    prestart: VecDeque<VideoFrame>,
    pending: Option<Prepared>,
    active: Option<Seg>,
    closing: Vec<Closing>,
    finisher: Option<(Sender<FinishJob>, JoinHandle<Vec<Finished>>)>,
    recreated: u32,
}

pub(crate) fn spawn(
    sh: Arc<Shared>,
    writers: Arc<dyn WriterFactory>,
    spec: VideoWriterSpec,
    rx: Receiver<RouterIn>,
) -> std::io::Result<JoinHandle<()>> {
    let (ftx, frx) = mpsc::channel();
    let fsh = sh.clone();
    let finisher = std::thread::Builder::new()
        .name("sc-record-finisher".into())
        .spawn(move || finisher(&fsh, frx))?;
    let frame_ns = 1_000_000_000 / i64::from(spec.fps.max(1));
    let mut r = Router {
        sh,
        writers,
        spec,
        frame_ns,
        start_host: None,
        prestart: VecDeque::new(),
        pending: None,
        active: None,
        closing: Vec::new(),
        finisher: Some((ftx, finisher)),
        recreated: 0,
    };
    std::thread::Builder::new()
        .name("sc-record-screen".into())
        .spawn(move || {
            while let Ok(m) = rx.recv() {
                match m {
                    RouterIn::Frame(f) => r.frame(f),
                    RouterIn::Cmd(c) => {
                        if r.cmd(c) {
                            return;
                        }
                    }
                }
            }
        })
}

fn append(sh: &Shared, seg: &mut Seg, f: &VideoFrame, file_ns: i64) {
    if file_ns <= seg.last_file_ns {
        return;
    }
    match seg.writer.append_video(f, file_ns) {
        Ok(Append::Written) => {
            bump(&seg.stats.frames);
            seg.last = Some(f.clone());
            seg.last_file_ns = file_ns;
        }
        Ok(Append::NotReady) => bump(&seg.stats.writer_not_ready),
        Err(e) => {
            bump(&seg.stats.dropped);
            seg.errors += 1;
            if seg.errors <= 3 {
                sh.rec.log(
                    Level::Error,
                    &format!("{}: append frame: {e}", seg.track_id),
                );
            }
        }
    }
}

fn content_px(src: &SourceDescriptor, f: &VideoFrame, size: SizePx) -> RectPx {
    let full = RectPx {
        x: 0,
        y: 0,
        w: size.w,
        h: size.h,
    };
    let r = match (f.content_rect_pt, f.content_scale) {
        (Some(r), Some(s)) => Some(pt_to_px(r, s)),
        _ => match src {
            SourceDescriptor::Region {
                region_pt,
                scale,
                method: RegionMethod::Crop,
                ..
            } => Some(pt_to_px(*region_pt, *scale)),
            _ => None,
        },
    };
    let Some(r) = r else { return full };
    let x = r.x.clamp(0, size.w as i32);
    let y = r.y.clamp(0, size.h as i32);
    let w = r.w.min(size.w - x as u32);
    let h = r.h.min(size.h - y as u32);
    if w == 0 || h == 0 {
        full
    } else {
        RectPx { x, y, w, h }
    }
}

impl Router {
    fn frame(&mut self, f: VideoFrame) {
        let for_pending = self.pending.as_ref().is_some_and(|p| p.tag == f.stream);
        let Some(start) = self.start_host else {
            if for_pending {
                if self.prestart.len() >= PRESTART_FRAMES {
                    self.prestart.pop_front();
                }
                self.prestart.push_back(f);
            }
            return;
        };
        if f.pts.0 >= self.sh.stop_host() {
            return;
        }
        if for_pending {
            if f.pts.0 >= start
                && let Some(p) = self.pending.take()
            {
                // The first segment's first frame is re-stamped to the recording start.
                let cut = if p.reason == StartReason::RecordStart {
                    start
                } else {
                    f.pts.0
                };
                self.open(p, f, cut);
            }
            return;
        }
        if let Some(seg) = self.active.as_mut().filter(|s| s.tag == f.stream) {
            let file_ns = f.pts.0 - start - seg.start_rec;
            append(&self.sh, seg, &f, file_ns);
            return;
        }
        if let Some(c) = self.closing.iter_mut().find(|c| c.seg.tag == f.stream) {
            if f.pts.0 >= c.cut_host {
                c.trailing += 1;
                if let Some(tx) = c.passed.take() {
                    let _ = tx.try_send(());
                }
            } else {
                let file_ns = f.pts.0 - start - c.seg.start_rec;
                append(&self.sh, &mut c.seg, &f, file_ns);
            }
        }
    }

    fn create(&self, path: &Path, size: SizePx) -> sc_platform::Result<Box<dyn TrackWriter>> {
        let spec = VideoWriterSpec {
            size,
            ..self.spec.clone()
        };
        self.writers.video(path, &spec)
    }

    fn open(&mut self, p: Prepared, f: VideoFrame, cut_host: i64) {
        let Some(start) = self.start_host else { return };
        let size = f.buffer.0.size();
        let path = self.sh.dir.join(&p.file);
        let writer = match p.writer {
            Some(w) if p.size == size => Ok(w),
            Some(w) => {
                let _ = w.finish(None);
                let _ = std::fs::remove_file(&path);
                self.recreated += 1;
                self.sh.rec.log(
                    Level::Info,
                    &format!(
                        "{}: first frame is {}x{}, not the prepared {}x{}; writer recreated",
                        p.seg, size.w, size.h, p.size.w, p.size.h
                    ),
                );
                self.create(&path, size)
            }
            None => {
                self.sh.rec.journal(&JournalEvent::SegmentPrepare {
                    seg: p.seg.clone(),
                    file: p.file.clone(),
                    host_ns: self.sh.now(),
                });
                hold(HoldPoint::AfterSegmentPrepare);
                self.create(&path, size)
            }
        };
        let writer = match writer {
            Ok(w) => w,
            Err(e) => {
                let error = format!("{}: create writer {}: {e}", p.seg, path.display());
                self.sh.rec.log(Level::Error, &error);
                self.sh.send(Ctl::OpenFailed { tag: p.tag, error });
                return;
            }
        };
        let cut_rec = cut_host - start;
        let content = content_px(&p.source, &f, size);
        self.sh.rec.journal(&JournalEvent::SegmentOpen {
            seg: p.seg.clone(),
            track_id: p.track_id.clone(),
            start_rec_ns: cut_rec,
            file: p.file.clone(),
            via: p.via,
            reason: p.reason,
            source: p.source.clone(),
            frame_px: size,
            content_px: content,
        });
        hold(HoldPoint::AfterSegmentOpen);
        let stats = self.sh.registry.add(&p.track_id, true);
        let mut seg = Seg {
            id: p.seg.clone(),
            track_id: p.track_id.clone(),
            tag: p.tag,
            writer,
            start_rec: cut_rec,
            last: None,
            last_file_ns: -1,
            stats: stats.clone(),
            errors: 0,
        };
        append(&self.sh, &mut seg, &f, 0);
        *lock(&self.sh.screen_stats) = Some(stats);
        let mut old_passed = None;
        let old_tag = self.active.replace(seg).map(|old| {
            let tag = old.tag;
            let (tx, rx) = mpsc::sync_channel(1);
            old_passed = Some(rx);
            self.closing.push(Closing {
                seg: old,
                cut_host,
                cut_rec,
                reason: p.closes_old_with,
                trailing: 0,
                passed: Some(tx),
            });
            tag
        });
        let file = p.file;
        let segment = ScreenSegment {
            id: p.seg,
            track_id: p.track_id,
            start_rec_ns: cut_rec,
            end_rec_ns: None,
            start_reason: p.reason,
            end_reason: None,
            via: p.via,
            source: p.source,
            frame_px: size,
            content_px: content,
            extra: Extra::new(),
        };
        self.sh.send(Ctl::Cut(Opened {
            tag: p.tag,
            segment,
            old_tag,
            old_passed,
            file,
        }));
    }

    fn close(&mut self, c: Closing) -> Closed {
        self.sh.rec.journal(&JournalEvent::SegmentClose {
            seg: c.seg.id.clone(),
            end_rec_ns: c.cut_rec,
            reason: c.reason,
            trailing_dropped: c.trailing,
        });
        let end_file_ns = (c.cut_rec - c.seg.start_rec).max(c.seg.last_file_ns + 1);
        let job = FinishJob {
            track_id: c.seg.track_id,
            writer: c.seg.writer,
            last: c.seg.last,
            last_file_ns: c.seg.last_file_ns,
            end_file_ns,
            frame_ns: self.frame_ns,
        };
        if let Some((tx, _)) = &self.finisher
            && let Err(mpsc::SendError(job)) = tx.send(job)
        {
            let _ = job.writer.finish(Some(job.end_file_ns));
        }
        Closed {
            seg: c.seg.id,
            end_rec_ns: c.cut_rec,
            reason: c.reason,
        }
    }

    fn end_active(&mut self, at_host: i64, reason: EndReason) -> Option<Closed> {
        let start = self.start_host?;
        let seg = self.active.take()?;
        *lock(&self.sh.screen_stats) = None;
        let end_rec = (at_host - start).max(seg.start_rec + seg.last_file_ns.max(0) + 1);
        Some(self.close(Closing {
            seg,
            cut_host: at_host,
            cut_rec: end_rec,
            reason,
            trailing: 0,
            passed: None,
        }))
    }

    fn cancel(&mut self, tag: StreamTag) -> bool {
        if self.pending.as_ref().is_none_or(|p| p.tag != tag) {
            return false;
        }
        if let Some(p) = self.pending.take() {
            self.drop_prepared(p);
        }
        true
    }

    fn drop_prepared(&self, p: Prepared) {
        if let Some(w) = p.writer {
            let _ = w.finish(None);
            let path = self.sh.dir.join(&p.file);
            let removed = std::fs::remove_file(&path);
            self.sh.rec.log(
                Level::Info,
                &format!(
                    "{}: prepared file {} discarded ({})",
                    p.seg,
                    p.file,
                    if removed.is_ok() {
                        "deleted"
                    } else {
                        "not found"
                    }
                ),
            );
        }
    }

    fn cmd(&mut self, c: RouterCmd) -> bool {
        match c {
            RouterCmd::Prepare(p) => {
                if let Some(old) = self.pending.replace(p) {
                    self.drop_prepared(old);
                }
            }
            RouterCmd::Begin { start_host } => {
                self.start_host = Some(start_host);
                let early = std::mem::take(&mut self.prestart);
                let (before, after): (Vec<_>, Vec<_>) =
                    early.into_iter().partition(|f| f.pts.0 < start_host);
                if let Some(f) = before.into_iter().last()
                    && let Some(p) = self.pending.take()
                {
                    self.open(p, f, start_host);
                }
                for f in after {
                    self.frame(f);
                }
            }
            RouterCmd::Cancel { tag, reply } => {
                let _ = reply.send(self.cancel(tag));
            }
            RouterCmd::OldStopped { tag, reply } => {
                let closed = match self.closing.iter().position(|c| c.seg.tag == tag) {
                    Some(i) => {
                        let c = self.closing.remove(i);
                        Some(self.close(c))
                    }
                    None => None,
                };
                let _ = reply.send(closed);
            }
            RouterCmd::Lose {
                tag,
                at_host,
                reply,
            } => {
                let closed = if self.active.as_ref().is_some_and(|s| s.tag == tag) {
                    self.end_active(at_host, EndReason::SourceLost)
                } else {
                    None
                };
                let _ = reply.send(closed);
            }
            RouterCmd::Stop { stop_host, reply } => {
                let mut closed = Vec::new();
                for c in std::mem::take(&mut self.closing) {
                    closed.push(self.close(c));
                }
                if let Some(c) = self.end_active(stop_host, EndReason::RecordStop) {
                    closed.push(c);
                }
                if let Some(p) = self.pending.take() {
                    self.drop_prepared(p);
                }
                let finished = match self.finisher.take() {
                    Some((tx, join)) => {
                        drop(tx);
                        join.join().unwrap_or_default()
                    }
                    None => Vec::new(),
                };
                let _ = reply.send(RouterDone {
                    closed,
                    finished,
                    recreated: self.recreated,
                });
                return true;
            }
        }
        false
    }
}

fn append_retry(w: &mut dyn TrackWriter, f: &VideoFrame, file_ns: i64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        match w.append_video(f, file_ns) {
            Ok(Append::Written) => return true,
            Ok(Append::NotReady) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1));
            }
            _ => return false,
        }
    }
}

fn finisher(sh: &Shared, rx: Receiver<FinishJob>) -> Vec<Finished> {
    let mut out = Vec::new();
    for mut job in rx {
        let hold_at = job.end_file_ns - job.frame_ns;
        if let Some(f) = &job.last
            && hold_at > job.last_file_ns
            && !append_retry(job.writer.as_mut(), f, hold_at)
        {
            sh.rec.log(
                Level::Warn,
                &format!("{}: could not append the held last frame", job.track_id),
            );
        }
        let r = job.writer.finish(Some(job.end_file_ns));
        match &r {
            Ok(t) => {
                sh.rec.journal(&JournalEvent::TrackFinished {
                    track_id: job.track_id.clone(),
                    duration_ns: t.duration_ns,
                    samples: t.samples,
                });
            }
            Err(e) => sh
                .rec
                .log(Level::Error, &format!("{}: finish: {e}", job.track_id)),
        }
        out.push((job.track_id, r.map_err(|e| e.to_string())));
    }
    out
}
