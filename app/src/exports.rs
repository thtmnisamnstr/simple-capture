//! The export sheet's back end (SPEC Export): settings remembered per format, named presets,
//! what this Mac can encode, an estimate before starting, and starting the export.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_export::plan::{ExportRequest, estimate, output_size, plan, reference};
use sc_export::settings::{ExportSettings, Format, VideoCodec, less_compatible};
use sc_export::{ExportJob, encode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::core::{Core, Reply};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportStore {
    pub last_format: Option<Format>,
    pub per_format: BTreeMap<String, ExportSettings>,
    pub presets: Vec<NamedSettings>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedSettings {
    pub name: String,
    pub settings: ExportSettings,
}

fn key(f: Format) -> String {
    serde_json::to_value(f)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn store_path(data: &Path) -> PathBuf {
    data.join("export.json")
}

pub fn load(data: &Path) -> ExportStore {
    std::fs::read(store_path(data))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(data: &Path, s: &ExportStore) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    sc_core::fsio::atomic_write(&store_path(data), &bytes).map_err(|e| e.to_string())
}

use crate::commands::arg;

impl Core {
    fn encoder_available(&self, name: &str) -> bool {
        self.probe_snapshot()
            .and_then(|p| p.encoder_ok(name))
            .unwrap_or(!name.ends_with("_videotoolbox") || cfg!(target_os = "macos"))
    }

    fn availability(&self) -> Value {
        let mut video = serde_json::Map::new();
        for v in [
            VideoCodec::H264,
            VideoCodec::Hevc,
            VideoCodec::Av1,
            VideoCodec::Vp9,
            VideoCodec::Vp8,
            VideoCodec::Prores,
            VideoCodec::Webp,
        ] {
            let k = serde_json::to_value(v).unwrap_or(Value::Null);
            let best = encode::video_encoder(v, sc_export::settings::Encoder::Best)
                .is_some_and(|e| self.encoder_available(e));
            let fast = encode::video_encoder(v, sc_export::settings::Encoder::Fast)
                .is_some_and(|e| self.encoder_available(e));
            video.insert(
                k.as_str().unwrap_or("").into(),
                json!({ "best": best, "fast": fast }),
            );
        }
        let gifski = sc_helpers::locate(sc_helpers::HelperKind::Gifski).is_ok();
        json!({ "video": video, "gifski": gifski })
    }

    pub fn export_action(self: &Arc<Self>, action: &str, args: &Value) -> Reply {
        match action {
            "export_options" => {
                let dir = self
                    .editor_dir()
                    .ok_or("no project is open in the editor")?;
                let project = sc_core::project::ProjectV1::load(&dir).map_err(|e| e.to_string())?;
                let (refsize, ratio) = reference(&project);
                Ok(json!({
                    "store": load(&self.data_dir),
                    "available": self.availability(),
                    "recording_fps": project.recording.screen.fps_requested,
                    "reference": refsize,
                    "ratio": [ratio.num, ratio.den],
                    "default_dir": dir.parent(),
                }))
            }
            "export_estimate" => {
                let settings: ExportSettings = arg(args, "settings")?;
                let dir = self
                    .editor_dir()
                    .ok_or("no project is open in the editor")?;
                self.editor_save_now();
                let project = sc_core::project::ProjectV1::load(&dir).map_err(|e| e.to_string())?;
                let req = ExportRequest {
                    project_dir: dir.clone(),
                    settings: settings.clone(),
                    out_path: None,
                    canvas: None,
                };
                let p =
                    plan(&project, &req, self.platform.local_now()).map_err(|e| e.to_string())?;
                let e = estimate(&p);
                let vt_limit = settings.encoder == sc_export::settings::Encoder::Fast
                    && settings.video == VideoCodec::H264
                    && p.canvas.w.max(p.canvas.h) > 4096;
                Ok(json!({
                    "canvas": p.canvas,
                    "fps": p.fps,
                    "frames": p.frames,
                    "duration_ns": p.duration_ns,
                    "file": p.out_path,
                    "bytes_low": e.bytes_low,
                    "bytes_high": e.bytes_high,
                    "seconds": e.seconds,
                    "audio": p.audio,
                    "less_compatible": less_compatible(settings.format, settings.video, settings.audio),
                    "too_large_for_fast": vt_limit,
                    "size_preview": output_size(&project, &settings),
                }))
            }
            "export_start" => {
                let settings: ExportSettings =
                    arg::<ExportSettings>(args, "settings")?.normalized();
                let dir = self
                    .editor_dir()
                    .ok_or("no project is open in the editor")?;
                let _ = self.editor_pause();
                self.editor_save_now();
                let mut store = load(&self.data_dir);
                store.last_format = Some(settings.format);
                store
                    .per_format
                    .insert(key(settings.format), settings.clone());
                save(&self.data_dir, &store)?;
                let platform = self.platform.clone();
                let d = dir.clone();
                self.run_export(&dir.to_string_lossy(), move |obs| {
                    ExportJob::start_with(
                        platform,
                        ExportRequest {
                            project_dir: d,
                            settings,
                            out_path: None,
                            canvas: None,
                        },
                        obs,
                    )
                })
            }
            "export_preset_save" => {
                let name: String = arg(args, "name")?;
                let settings: ExportSettings = arg(args, "settings")?;
                let mut store = load(&self.data_dir);
                store.presets.retain(|p| p.name != name);
                store.presets.push(NamedSettings { name, settings });
                save(&self.data_dir, &store)?;
                Ok(json!(store))
            }
            "export_preset_delete" => {
                let name: String = arg(args, "name")?;
                let mut store = load(&self.data_dir);
                store.presets.retain(|p| p.name != name);
                save(&self.data_dir, &store)?;
                Ok(json!(store))
            }
            "choose_folder" => {
                let (tx, rx) = std::sync::mpsc::channel();
                self.platform.desktop().choose_folder(Box::new(move |p| {
                    let _ = tx.send(p);
                }));
                let p = rx
                    .recv()
                    .map_err(|_| "the folder chooser went away".to_string())?;
                Ok(p.map(|p| json!(p)).unwrap_or(Value::Null))
            }
            "copy_file" => {
                let path: String = arg(args, "path")?;
                self.platform
                    .desktop()
                    .copy_file(Path::new(&path))
                    .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "open_file" => {
                let path: String = arg(args, "path")?;
                crate::core::open_path(&path)?;
                Ok(Value::Null)
            }
            other => Err(format!("unknown export action {other:?}")),
        }
    }
}
