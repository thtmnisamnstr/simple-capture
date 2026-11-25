use std::path::{Path, PathBuf};
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
use crate::encode;
use crate::plan::{ExportRequest, Plan};
use crate::recipe::{FrameRenderer, quick_canvas};
use crate::settings::{ExportSettings, Format, GifMode};

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
    /// The quick export (MP4, H.264, Balanced) at `req.size`.
    pub fn start(
        platform: Arc<dyn Platform>,
        req: QuickExportRequest,
        obs: Arc<dyn ExportObserver>,
    ) -> Result<ExportJob> {
        let project = ProjectV1::load(&req.project_dir).map_err(|e| {
            Error::Other(format!(
                "sc_export: load {}: {e}",
                req.project_dir.display()
            ))
        })?;
        let mut settings = ExportSettings::for_format(Format::Mp4);
        if let Some(f) = req.fps {
            settings.fps = f;
        }
        let out_path = match req.out_path {
            Some(p) => p,
            None => crate::recipe::default_out_path(&req.project_dir)?,
        };
        ExportJob::start_with(
            platform,
            ExportRequest {
                canvas: Some(quick_canvas(&project, req.size)),
                project_dir: req.project_dir,
                settings,
                out_path: Some(out_path),
            },
            obs,
        )
    }

    /// Loads and plans synchronously (errors come back here), then renders on a worker thread.
    pub fn start_with(
        platform: Arc<dyn Platform>,
        req: ExportRequest,
        obs: Arc<dyn ExportObserver>,
    ) -> Result<ExportJob> {
        ExportJob::start_staged(platform, req, None, obs)
    }

    /// [`ExportJob::start_with`], except that with `stage` set the helper writes the file and
    /// its temporary files in that folder, and the finished file then moves to the planned path.
    /// A sandboxed helper reaches only the folders the app's entitlements name, not a folder the
    /// person let the app use at run time.
    pub fn start_staged(
        platform: Arc<dyn Platform>,
        req: ExportRequest,
        stage: Option<PathBuf>,
        obs: Arc<dyn ExportObserver>,
    ) -> Result<ExportJob> {
        obs.on_state(&ExportState::Preparing);
        let project = ProjectV1::load(&req.project_dir).map_err(|e| {
            Error::Other(format!(
                "sc_export: load {}: {e}",
                req.project_dir.display()
            ))
        })?;
        let plan = crate::plan::plan(&project, &req, platform.local_now())?;
        if plan.settings.gif.mode != req.settings.gif.mode {
            tracing::info!("no gifski helper; the GIF uses FFmpeg's palette");
        }
        if let Some(parent) = plan.out_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Other(format!("sc_export: {}: {e}", parent.display())))?;
        }
        let dest = PartFile::create(&plan.out_path)?;
        let (part, dest) = match &stage {
            None => (dest, None),
            Some(dir) => (
                PartFile::create(&staged_path(dir, &plan.out_path)?)?,
                Some(dest),
            ),
        };
        let control = Arc::new(Control::default());
        let c = control.clone();
        let worker = std::thread::Builder::new()
            .name("sc-export".into())
            .spawn(move || {
                let w = Worker {
                    platform,
                    dir: req.project_dir,
                    project,
                    plan,
                    control: c,
                    obs: obs.clone(),
                    t0: Instant::now(),
                    last: Mutex::new(None),
                };
                let r = w.run(part, dest);
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
    dir: PathBuf,
    project: ProjectV1,
    plan: Plan,
    control: Arc<Control>,
    obs: Arc<dyn ExportObserver>,
    t0: Instant,
    last: Mutex<Option<Instant>>,
}

impl Worker {
    fn check_cancel(&self) -> Result<()> {
        if self.control.canceled() {
            return Err(Error::Other(CANCELED.into()));
        }
        Ok(())
    }

    fn run(&self, part: PartFile, dest: Option<PartFile>) -> Result<PathBuf> {
        let plan = &self.plan;
        let st = &plan.settings;
        let mut wav_part = None;
        if plan.audio {
            let wav_final = {
                let mut s = part.final_path().as_os_str().to_owned();
                s.push(".wav");
                PathBuf::from(s)
            };
            let wp = PartFile::create(&wav_final)?;
            let wrote = mix_to_wav_cancelable(
                &self.project,
                &self.dir,
                self.platform.decoder(),
                plan.fps,
                plan.frames,
                wp.path(),
                &self.control.cancel,
            );
            self.check_cancel()?;
            if wrote? {
                wav_part = Some(wp);
            }
        }
        self.check_cancel()?;
        let mut renderer = FrameRenderer::new(
            &self.project,
            &self.dir,
            self.platform.decoder(),
            plan.canvas,
            Backend::Auto,
            OutputFormat::I420Bt709Limited,
        )?;
        let passes: u64 = if st.format == Format::Gif && st.gif.mode == GifMode::Palette {
            2
        } else {
            1
        };
        if st.format == Format::Gif {
            if st.gif.mode == GifMode::Gifski {
                let gifski = sc_helpers::locate(HelperKind::Gifski)?;
                let args = encode::gifski_args(st, plan.fps, part.path());
                self.encode(gifski, args, &mut renderer, 0, passes)?;
            } else {
                let ffmpeg = sc_helpers::locate(HelperKind::Ffmpeg)?;
                let palette = PartFile::create(&{
                    let mut s = part.final_path().as_os_str().to_owned();
                    s.push(".palette");
                    PathBuf::from(s)
                })?;
                let gen_args = encode::palette_gen_args(st, palette.path());
                self.encode(ffmpeg.clone(), gen_args, &mut renderer, 0, passes)?;
                renderer = FrameRenderer::new(
                    &self.project,
                    &self.dir,
                    self.platform.decoder(),
                    plan.canvas,
                    Backend::Auto,
                    OutputFormat::I420Bt709Limited,
                )?;
                let use_args = encode::palette_use_args(st, palette.path(), part.path());
                self.encode(ffmpeg, use_args, &mut renderer, 1, passes)?;
                drop(palette);
            }
        } else {
            let ffmpeg = sc_helpers::locate(HelperKind::Ffmpeg)?;
            let no_encoder = || {
                Error::Other(format!(
                    "sc_export: no encoder for {:?} {:?} here",
                    st.video, st.encoder
                ))
            };
            let pick = encode::pick_video_encoder(
                st,
                plan.canvas,
                plan.fps,
                |name| crate::probe::encoder_available(&ffmpeg, name),
                || crate::probe::vt_h264_opens(&ffmpeg, plan.canvas),
            )
            .ok_or_else(no_encoder)?;
            self.check_cancel()?;
            if Some(pick.encoder) != encode::video_encoder(st.video, st.encoder)
                || pick.software_kbps.is_some()
            {
                tracing::info!(
                    encoder = pick.encoder,
                    software_kbps = ?pick.software_kbps,
                    "export encoder fallback"
                );
            }
            let wav = wav_part.as_ref().map(|w| w.path());
            let args =
                encode::video_args_with(st, pick, wav, part.path()).ok_or_else(no_encoder)?;
            self.encode(ffmpeg, args, &mut renderer, 0, passes)?;
        }
        self.check_cancel()?;
        drop(wav_part);
        let Some(dest) = dest else {
            return Ok(part.commit()?);
        };
        move_over(part.path(), dest.path())?;
        drop(part);
        Ok(dest.commit()?)
    }

    fn encode(
        &self,
        program: PathBuf,
        args: Vec<String>,
        renderer: &mut FrameRenderer,
        pass: u64,
        passes: u64,
    ) -> Result<()> {
        let plan = &self.plan;
        let frame_bytes = y4m::FRAME.len() + y4m::i420_frame_bytes(plan.canvas);
        let helper = HelperProcess::spawn(SpawnSpec {
            program,
            args,
            frame_bytes,
            queue_frames: sc_helpers::DEFAULT_QUEUE_FRAMES,
        })?;
        *self
            .control
            .interrupter
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(helper.interrupter());
        if let Err(e) = self.feed(&helper, renderer, pass, passes) {
            let r = helper.cancel();
            tracing::info!(?r, "export helper stopped");
            return Err(if self.control.canceled() {
                Error::Other(CANCELED.into())
            } else {
                e
            });
        }
        if pass + 1 == passes {
            self.obs.on_state(&ExportState::Finalizing);
        }
        let report = helper.finish()?;
        if !report.success() {
            return Err(Error::Other(format!(
                "sc_export: helper exited with {:?}: {}",
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
        Ok(())
    }

    fn feed(
        &self,
        helper: &HelperProcess,
        renderer: &mut FrameRenderer,
        pass: u64,
        passes: u64,
    ) -> Result<()> {
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
        let total = plan.frames * passes;
        let mut done = plan.frames * pass;
        for t in cfr_frame_times(plan.duration_ns, plan.fps) {
            self.check_cancel()?;
            let (frame, _) = renderer.render(t)?;
            helper.write_frame_parts(&[y4m::FRAME, &frame.data])?;
            done += 1;
            let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
            if done == total || last.is_none_or(|l| l.elapsed() >= PROGRESS_EVERY) {
                *last = Some(Instant::now());
                let secs = self.t0.elapsed().as_secs_f64().max(1e-9);
                let fps = done as f64 / secs;
                self.obs.on_progress(&ExportProgress {
                    frames_done: done,
                    frames_total: total,
                    fps,
                    eta_s: (fps > 0.0).then(|| (total - done) as f64 / fps),
                });
            }
        }
        Ok(())
    }
}

fn staged_path(stage: &Path, out: &Path) -> Result<PathBuf> {
    let (Some(stem), Some(ext)) = (out.file_stem(), out.extension()) else {
        return Err(Error::Other(format!(
            "sc_export: {} has no file name",
            out.display()
        )));
    };
    crate::plan::free_path(
        stage,
        &format!(".export {}", stem.to_string_lossy()),
        &ext.to_string_lossy(),
    )
}

fn move_over(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map(|_| ()).map_err(|e| {
        Error::Other(format!(
            "sc_export: copy {} to {}: {e}",
            from.display(),
            to.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_names_are_hidden_and_free() {
        let dir = std::env::temp_dir().join(format!("sc-stage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let out = Path::new("/elsewhere/My Edit.mp4");
        let first = staged_path(&dir, out).expect("free");
        assert_eq!(first, dir.join(".export My Edit.mp4"));
        let held = PartFile::create(&first).expect("part");
        assert_eq!(
            staged_path(&dir, out).expect("free"),
            dir.join(".export My Edit 2.mp4")
        );
        drop(held);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_over_replaces_the_reserved_part() {
        let dir = std::env::temp_dir().join(format!("sc-move-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let from = dir.join("a.part");
        let to = dir.join("b.part");
        std::fs::write(&from, b"encoded").expect("write");
        std::fs::write(&to, b"").expect("write");
        move_over(&from, &to).expect("move");
        assert!(!from.exists());
        assert_eq!(std::fs::read(&to).expect("read"), b"encoded");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
