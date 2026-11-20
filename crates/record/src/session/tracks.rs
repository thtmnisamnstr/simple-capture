//! One writer thread per continuous track (camera, mic, system audio).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sc_core::journal::{GapKind, JournalEvent};
use sc_core::project::{Extra, GapRecord, TrackKind};
use sc_core::sessionlog::Level;
use sc_platform::{Append, AudioBuffer, AudioTrackKind, FinishedTrack, TrackWriter, VideoFrame};

use super::Notice;
use super::gapfill::GapFiller;
use super::shared::{Shared, TrackStats, bump};
use super::sink::TrackIn;

pub(crate) struct TrackResult {
    pub track_id: String,
    pub first_host: Option<i64>,
    pub finished: Option<Result<FinishedTrack, String>>,
    pub gaps: Vec<GapRecord>,
    pub stats: Arc<TrackStats>,
}

pub(crate) struct TrackCtx {
    pub sh: Arc<Shared>,
    pub track_id: String,
    pub kind: TrackKind,
    pub writer: Box<dyn TrackWriter>,
    pub stats: Arc<TrackStats>,
}

struct Worker {
    c: TrackCtx,
    first: Option<i64>,
    last_video: i64,
    filler: Option<GapFiller>,
    gaps: Vec<GapRecord>,
    errors: u32,
    last_gap_notice: Option<Instant>,
}

pub(crate) fn spawn(
    c: TrackCtx,
    rx: Receiver<TrackIn>,
) -> std::io::Result<JoinHandle<TrackResult>> {
    let name = format!("sc-record-{}", c.track_id);
    let mut w = Worker {
        c,
        first: None,
        last_video: -1,
        filler: None,
        gaps: Vec::new(),
        errors: 0,
        last_gap_notice: None,
    };
    std::thread::Builder::new().name(name).spawn(move || {
        while let Ok(m) = rx.recv() {
            match m {
                TrackIn::Video(f) => w.video(&f),
                TrackIn::Audio(a) => w.audio(&a),
                TrackIn::Finish { end_host } => return w.finish(end_host),
            }
        }
        w.finish(None)
    })
}

impl Worker {
    fn error(&mut self, what: &str, e: &sc_platform::Error) {
        bump(&self.c.stats.dropped);
        self.errors += 1;
        if self.errors <= 3 {
            self.c
                .sh
                .rec
                .log(Level::Error, &format!("{}: {what}: {e}", self.c.track_id));
        }
    }

    fn video(&mut self, f: &VideoFrame) {
        if f.pts.0 < self.c.sh.start_host() {
            return;
        }
        let first = *self.first.get_or_insert(f.pts.0);
        let file_ns = f.pts.0 - first;
        if file_ns <= self.last_video {
            bump(&self.c.stats.dropped);
            return;
        }
        match self.c.writer.append_video(f, file_ns) {
            Ok(Append::Written) => {
                bump(&self.c.stats.frames);
                self.last_video = file_ns;
            }
            Ok(Append::NotReady) => bump(&self.c.stats.writer_not_ready),
            Err(e) => self.error("append frame", &e),
        }
    }

    /// Audio is never dropped silently: a buffer the writer cannot take is not committed, so the
    /// next one shows up as a gap and is preceded by silence.
    fn write(
        &mut self,
        what: &str,
        f: impl Fn(&mut dyn TrackWriter) -> sc_platform::Result<Append>,
    ) -> bool {
        let deadline = Instant::now() + Duration::from_millis(20);
        loop {
            match f(self.c.writer.as_mut()) {
                Ok(Append::Written) => return true,
                Ok(Append::NotReady) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(Append::NotReady) => {
                    bump(&self.c.stats.writer_not_ready);
                    return false;
                }
                Err(e) => {
                    self.error(what, &e);
                    return false;
                }
            }
        }
    }

    fn audio(&mut self, a: &AudioBuffer) {
        let start = self.c.sh.start_host();
        if a.pts.0 < start {
            return;
        }
        let threshold = self.c.sh.tuning.gap_threshold_ns;
        let step = self
            .filler
            .get_or_insert_with(|| GapFiller::new(a.sample_rate, threshold))
            .plan(a.pts.0, a.frames);
        if step.silence_frames > 0 {
            if !self.write("append silence", |w| {
                w.append_silence(step.silence_file_ns, step.silence_frames)
            }) {
                return;
            }
            if let Some(g) = self.filler.as_mut() {
                g.commit_silence(&step);
            }
        }
        let written = step.drop
            || self.write("append audio", |w| {
                w.append_audio(a, step.buffer_file_ns, step.skip_frames)
            });
        if !written {
            return;
        }
        if let Some(g) = self.filler.as_mut() {
            g.commit_buffer(&step);
            self.c.stats.drift_ns.store(g.drift_ns(), Ordering::Relaxed);
            if self.first.is_none() {
                self.first = g.anchor();
            }
        }
        if !step.drop {
            self.c
                .stats
                .frames
                .fetch_add(u64::from(a.frames - step.skip_frames), Ordering::Relaxed);
        }
        let Some(gap) = step.gap else { return };
        let at_rec_ns = gap.at_host_ns - start;
        self.c.sh.rec.journal(&JournalEvent::AudioGap {
            track: self.c.track_id.clone(),
            kind: gap.kind,
            at_rec_ns,
            dur_ns: gap.dur_ns,
        });
        if gap.kind == GapKind::Overlap {
            self.c.stats.overlaps.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.c.stats.gaps.fetch_add(1, Ordering::Relaxed);
        self.gaps.push(GapRecord {
            at_rec_ns,
            dur_ns: gap.dur_ns,
            extra: Extra::new(),
        });
        let quiet = self
            .last_gap_notice
            .is_some_and(|t| t.elapsed() < Duration::from_secs(5));
        if !quiet {
            self.last_gap_notice = Some(Instant::now());
            let track = if self.c.kind == TrackKind::Mic {
                AudioTrackKind::Mic
            } else {
                AudioTrackKind::SystemAudio
            };
            self.c.sh.notice(Notice::AudioGap {
                track,
                dur_ns: gap.dur_ns,
            });
        }
    }

    fn end_file_ns(&self, end_host: Option<i64>) -> Option<i64> {
        let written = self
            .filler
            .as_ref()
            .and_then(|g| Some(g.expected_host()? - g.anchor()?));
        file_end(end_host, self.first, written)
    }

    fn finish(self, end_host: Option<i64>) -> TrackResult {
        let end_file_ns = self.end_file_ns(end_host);
        let Worker { c, first, gaps, .. } = self;
        let r = c.writer.finish(end_file_ns);
        match &r {
            Ok(t) => {
                c.sh.rec.journal(&JournalEvent::TrackFinished {
                    track_id: c.track_id.clone(),
                    duration_ns: t.duration_ns,
                    samples: t.samples,
                });
            }
            Err(e) => {
                c.sh.rec
                    .log(Level::Error, &format!("{}: finish: {e}", c.track_id))
            }
        }
        TrackResult {
            track_id: c.track_id,
            first_host: first,
            finished: Some(r.map_err(|e| e.to_string())),
            gaps,
            stats: c.stats,
        }
    }
}

/// Devices keep delivering for a moment after the stop request (the mic ran 150 ms past it on
/// the M3 Pro), so a file ends at the stop time, or at the last audio written if that is earlier.
fn file_end(end_host: Option<i64>, first_host: Option<i64>, written: Option<i64>) -> Option<i64> {
    let end = end_host? - first_host?;
    if end <= 0 {
        return None;
    }
    Some(written.map_or(end, |w| end.min(w)))
}

#[cfg(test)]
mod tests {
    use super::file_end;

    /// Regression (wp-e2e-cli): a 4.019 s recording had a 4.171 s mic file because samples
    /// delivered after the stop were kept.
    #[test]
    fn files_end_at_the_stop() {
        let first = 1_000_000_000 + 8_000_000;
        let stop = 1_000_000_000 + 4_018_630_500;
        assert_eq!(
            file_end(Some(stop), Some(first), Some(4_170_666_667)),
            Some(stop - first)
        );
        assert_eq!(
            file_end(Some(stop), Some(first), Some(3_980_000_000)),
            Some(3_980_000_000)
        );
        assert_eq!(file_end(Some(stop), Some(first), None), Some(stop - first));
        assert_eq!(file_end(None, Some(first), Some(5)), None);
        assert_eq!(file_end(Some(stop), None, None), None);
        assert_eq!(file_end(Some(first), Some(stop), None), None);
    }
}
