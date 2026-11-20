//! Recording session (plan §4.5, ARCHITECTURE §4–§6, §12).
//!
//! Threads: control (commands, events, switch controller, `project.json`), screen writer/router,
//! finisher, one writer per continuous track, health (1 Hz). Capture callbacks only `try_send`
//! into bounded queues.

mod control;
pub mod gapfill;
mod health;
mod router;
mod shared;
mod sink;
mod tracks;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use sc_core::journal::StopReason;
use sc_core::project::{Canvas, SystemAudioMode, Via};
use sc_platform::{AudioTrackKind, DeviceKind, Platform, Resolution, SourceSpec, WindowId};

use crate::{Error, Result};
use shared::{Ctl, Shared, lock};

#[derive(Debug, Clone)]
pub struct RecordRequest {
    pub projects_dir: PathBuf,
    pub title: String,
    pub source: SourceSpec,
    pub via: Via,
    pub fps: u32,
    pub resolution: Resolution,
    pub show_cursor: bool,
    pub click_highlight: bool,
    pub camera: Option<String>,
    pub mic: Option<String>,
    pub system_audio: SystemAudioMode,
    /// For aspect Original, `ratio` is overwritten when s0 opens.
    pub canvas: Canvas,
    pub background_image: Option<PathBuf>,
    pub exclude: Vec<WindowId>,
    pub tuning: Tuning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tuning {
    pub screen_queue: usize,
    pub camera_queue: usize,
    pub audio_queue: usize,
    pub gap_threshold_ns: i64,
    pub start_timeout_ms: u32,
    pub switch_first_frame_timeout_ms: u32,
    pub frames_dropping_pct: f64,
    pub disk_warn_bytes: u64,
    pub disk_stop_bytes: u64,
    pub fragment_interval_s: f64,
}

impl Default for Tuning {
    /// Reasons are in the wp-record-session report (DECISIONS.md).
    fn default() -> Self {
        Tuning {
            screen_queue: 3,
            camera_queue: 8,
            audio_queue: 256,
            gap_threshold_ns: 2_000_000,
            start_timeout_ms: 5_000,
            switch_first_frame_timeout_ms: 1_500,
            frames_dropping_pct: 2.0,
            disk_warn_bytes: 2_000_000_000,
            disk_stop_bytes: 500_000_000,
            fragment_interval_s: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    Preparing,
    Recording,
    Switching,
    Stopping,
    Finalized { project_dir: PathBuf },
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    SwitchFailed {
        error: String,
        restored: bool,
    },
    SourceLost {
        segment: String,
    },
    DeviceLost {
        kind: DeviceKind,
        name: String,
    },
    DiskLow {
        bytes_left: u64,
    },
    AudioGap {
        track: AudioTrackKind,
        dur_ns: i64,
    },
    /// Over the last 10 s; the UI suggests 30 fps or 1×.
    FramesDropping {
        track: String,
        pct: f64,
    },
    StoppedForSleep,
    StoppedForDisk,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackHealthSnap {
    pub track_id: String,
    pub frames: u64,
    pub dropped: u64,
    pub writer_not_ready: u64,
    pub gaps: u32,
    pub overlaps: u32,
    pub fps: f64,
    /// Audio: count-based end − host-time end.
    pub drift_ns: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HealthSnapshot {
    pub rec_ns: i64,
    pub tracks: Vec<TrackHealthSnap>,
    pub free_disk_bytes: u64,
}

pub trait SessionObserver: Send + Sync {
    fn on_state(&self, s: &SessionState);
    fn on_health(&self, h: &HealthSnapshot);
    fn on_notice(&self, n: &Notice);
}

#[derive(Debug, Clone, PartialEq)]
pub struct StopSummary {
    pub project_dir: PathBuf,
    pub duration_ns: i64,
    pub segments: usize,
    pub health: HealthSnapshot,
}

pub struct Session {
    project_dir: PathBuf,
    sh: Arc<Shared>,
    ctl: Sender<Ctl>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("project_dir", &self.project_dir)
            .finish_non_exhaustive()
    }
}

const ENDED: &str = "the recording has ended";

impl Session {
    /// Returns once every requested source delivered a sample and s0 is about to open; fails
    /// before any project folder exists when a permission is missing.
    pub fn start(
        platform: Arc<dyn Platform>,
        req: RecordRequest,
        obs: Arc<dyn SessionObserver>,
    ) -> Result<Session> {
        let started = control::start(platform, req, obs)?;
        Ok(Session {
            project_dir: started.sh.dir.clone(),
            ctl: started.sh.ctl.clone(),
            sh: started.sh,
            thread: Some(started.thread),
        })
    }

    fn check_live(&self) -> Result<()> {
        match self.state() {
            SessionState::Preparing | SessionState::Recording | SessionState::Switching => Ok(()),
            s => Err(Error::State(format!("{ENDED} ({s:?})"))),
        }
    }

    /// Async; the result arrives through state and notices.
    pub fn switch_source(&self, spec: SourceSpec, via: Via) -> Result<()> {
        self.check_live()?;
        self.ctl
            .send(Ctl::Switch(spec, via))
            .map_err(|_| Error::State(ENDED.into()))
    }

    /// M1: pause/resume is M7.
    pub fn pause(&self) -> Result<()> {
        Err(Error::Unsupported("pause/resume is M7"))
    }

    /// M1: pause/resume is M7.
    pub fn resume(&self) -> Result<()> {
        Err(Error::Unsupported("pause/resume is M7"))
    }

    pub fn set_exclusions(&self, exclude: Vec<WindowId>) -> Result<()> {
        self.check_live()?;
        let (tx, rx) = mpsc::channel();
        self.ctl
            .send(Ctl::SetExclusions(exclude, tx))
            .map_err(|_| Error::State(ENDED.into()))?;
        rx.recv().map_err(|_| Error::State(ENDED.into()))?
    }

    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    pub fn state(&self) -> SessionState {
        lock(&self.sh.state).clone()
    }

    /// After the session stopped itself (sleep, disk), returns that recording's summary.
    pub fn stop(mut self, reason: StopReason) -> Result<StopSummary> {
        self.stop_inner(reason)
    }

    pub fn discard(mut self) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let sent = self.ctl.send(Ctl::Discard(tx)).is_ok();
        let r = if sent {
            rx.recv().map_err(|_| Error::State(ENDED.into()))?
        } else {
            Err(Error::State(ENDED.into()))
        };
        self.join();
        r
    }

    fn stop_inner(&mut self, reason: StopReason) -> Result<StopSummary> {
        let (tx, rx) = mpsc::channel();
        let r = match self.ctl.send(Ctl::Stop(reason, tx)) {
            Ok(()) => rx.recv().map_err(|_| Error::State(ENDED.into()))?,
            Err(_) => Err(Error::State(ENDED.into())),
        };
        self.join();
        r
    }

    fn join(&mut self) {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let _ = self.stop_inner(StopReason::Quit);
        }
    }
}
