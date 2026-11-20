use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sc_core::project::ProjectV1;
use sc_decode::cfr_frame_times;
use sc_geometry::SizeClass;
use sc_helpers::part::PartFile;
use sc_helpers::y4m::{self, Y4mHeader};
use sc_helpers::{HelperKind, HelperProcess, Interrupter, SpawnSpec};
use sc_platform::{Error, Platform, Result};
use sc_render::{Backend, OutputFormat};

use crate::audio::mix_to_wav_cancelable;
use crate::recipe::{FrameRenderer, QuickPlan, quick_plan, x264_quick};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportSize {
    Reference,
    Class(SizeClass),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickExportRequest {
    pub project_dir: PathBuf,
    pub out_path: Option<PathBuf>,
    pub size: ExportSize,
    pub fps: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportState {
    Preparing,
    Rendering,
    Finalizing,
    Done { path: PathBuf },
    Canceled,
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportProgress {
    pub frames_done: u64,
    pub frames_total: u64,
    pub fps: f64,
    pub eta_s: Option<f64>,
}

pub trait ExportObserver: Send + Sync {
    fn on_state(&self, s: &ExportState);
    fn on_progress(&self, p: &ExportProgress);
}

const PROGRESS_EVERY: Duration = Duration::from_millis(100);
pub(crate) const CANCELED: &str = "sc_export: export canceled";

#[derive(Default)]
struct Control {
    cancel: AtomicBool,
    interrupter: Mutex<Option<Interrupter>>,
}

impl Control {
    fn canceled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

pub struct ExportJob {
    control: Arc<Control>,
    worker: JoinHandle<Result<PathBuf>>,
}

impl std::fmt::Debug for ExportJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportJob")
            .field("canceled", &self.control.canceled())
            .finish_non_exhaustive()
    }
}

impl ExportJob {
    /// Loads and plans synchronously (errors come back here), then renders on a worker thread.
    pub fn start(
        platform: Arc<dyn Platform>,
        req: QuickExportRequest,
        obs: Arc<dyn ExportObserver>,
    ) -> Result<ExportJob> {
        obs.on_state(&ExportState::Preparing);
        let project = ProjectV1::load(&req.project_dir).map_err(|e| {
            Error::Other(format!(
                "sc_export: load {}: {e}",
                req.project_dir.display()
            ))
        })?;
        let plan = quick_plan(&project, &req.project_dir, &req)?;
        let part = PartFile::create(&plan.out_path)?;
        let control = Arc::new(Control::default());
        let c = control.clone();
        let worker = std::thread::Builder::new()
            .name("sc-export".into())
            .spawn(move || {
                let w = Worker {
                    platform,
                    req,
                    project,
                    plan,
                    control: c,
                    obs: obs.clone(),
                };
                let r = w.run(part);
                match &r {
                    Ok(path) => obs.on_state(&ExportState::Done { path: path.clone() }),
                    Err(Error::Other(m)) if m == CANCELED => obs.on_state(&ExportState::Canceled),
                    Err(e) => obs.on_state(&ExportState::Failed {
                        error: e.to_string(),
                    }),
                }
                r
            })
            .map_err(|e| Error::Other(format!("sc_export: start worker: {e}")))?;
        Ok(ExportJob { control, worker })
    }

    /// Stops the export; the `.part`, the mix WAV and the helper are gone once `wait` returns.
    pub fn cancel(&self) {
        self.control.cancel.store(true, Ordering::SeqCst);
        let i = self
            .control
            .interrupter
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(i) = i.as_ref() {
            i.interrupt();
        }
    }

    /// The final path, or `Err(Other("sc_export: export canceled"))` after a cancel.
    pub fn wait(self) -> Result<PathBuf> {
        self.worker
            .join()
            .map_err(|_| Error::Other("sc_export: export worker panicked".into()))?
    }
}

struct Worker {
    platform: Arc<dyn Platform>,
    req: QuickExportRequest,
    project: ProjectV1,
    plan: QuickPlan,
    control: Arc<Control>,
    obs: Arc<dyn ExportObserver>,
}

impl Worker {
    fn check_cancel(&self) -> Result<()> {
        if self.control.canceled() {
            return Err(Error::Other(CANCELED.into()));
        }
        Ok(())
    }

    fn run(&self, part: PartFile) -> Result<PathBuf> {
        let plan = &self.plan;
        let dir = &self.req.project_dir;
        let wav_final = {
            let mut s = plan.out_path.as_os_str().to_owned();
            s.push(".wav");
            PathBuf::from(s)
        };
        let mut wav_part = None;
        if !plan.audio_tracks.is_empty() {
            let wp = PartFile::create(&wav_final)?;
            let wrote = mix_to_wav_cancelable(
                &self.project,
                dir,
                self.platform.decoder(),
                plan.fps,
                plan.frames,
                wp.path(),
                &self.control.cancel,
            );
            if self.control.canceled() {
                return Err(Error::Other(CANCELED.into()));
            }
            if wrote? {
                wav_part = Some(wp);
            }
        }
        self.check_cancel()?;
        let mut renderer = FrameRenderer::new(
            &self.project,
            dir,
            self.platform.decoder(),
            plan.canvas,
            Backend::Auto,
            OutputFormat::I420Bt709Limited,
        )?;
        let ffmpeg = sc_helpers::locate(HelperKind::Ffmpeg)?;
        let frame_bytes = y4m::FRAME.len() + y4m::i420_frame_bytes(plan.canvas);
        let helper = HelperProcess::spawn(SpawnSpec {
            program: ffmpeg,
            args: x264_quick(plan.canvas, plan.fps, wav_part.is_some(), part.path()),
            frame_bytes,
            queue_frames: sc_helpers::DEFAULT_QUEUE_FRAMES,
        })?;
        *self
            .control
            .interrupter
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(helper.interrupter());
        match self.feed(&helper, &mut renderer) {
            Ok(()) => {}
            Err(e) => {
                let r = helper.cancel();
                tracing::info!(?r, "export helper stopped");
                return Err(if self.control.canceled() {
                    Error::Other(CANCELED.into())
                } else {
                    e
                });
            }
        }
        self.obs.on_state(&ExportState::Finalizing);
        let report = helper.finish()?;
        if !report.success() {
            return Err(Error::Other(format!(
                "sc_export: ffmpeg exited with {:?}: {}",
                report.code,
                report.stderr_tail.join(" | ")
            )));
        }
        if report.frames_written != plan.frames {
            return Err(Error::Other(format!(
                "sc_export: wrote {} of {} frames",
                report.frames_written, plan.frames
            )));
        }
        self.check_cancel()?;
        drop(wav_part);
        Ok(part.commit()?)
    }

    fn feed(&self, helper: &HelperProcess, renderer: &mut FrameRenderer) -> Result<()> {
        let plan = &self.plan;
        helper.write_bytes(
            y4m::header(&Y4mHeader {
                size: plan.canvas,
                fps_num: plan.fps,
                fps_den: 1,
            })
            .as_bytes(),
        )?;
        self.obs.on_state(&ExportState::Rendering);
        let t0 = Instant::now();
        let mut last = None::<Instant>;
        let mut done = 0u64;
        for t in cfr_frame_times(plan.duration_ns, plan.fps) {
            self.check_cancel()?;
            let (frame, _) = renderer.render(t)?;
            helper.write_frame_parts(&[y4m::FRAME, &frame.data])?;
            done += 1;
            if done == plan.frames || last.is_none_or(|l| l.elapsed() >= PROGRESS_EVERY) {
                last = Some(Instant::now());
                let secs = t0.elapsed().as_secs_f64().max(1e-9);
                let fps = done as f64 / secs;
                self.obs.on_progress(&ExportProgress {
                    frames_done: done,
                    frames_total: plan.frames,
                    fps,
                    eta_s: (fps > 0.0).then(|| (plan.frames - done) as f64 / fps),
                });
            }
        }
        Ok(())
    }
}
