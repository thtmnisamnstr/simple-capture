//! The recordings in the projects folder, newest first (SPEC Library and safety).

use std::path::{Path, PathBuf};

use sc_core::project::{ProjectState, ProjectV1};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    pub dir: PathBuf,
    pub title: String,
    pub created_at: String,
    pub state: ProjectState,
    pub duration_ns: Option<i64>,
    pub edited_ns: i64,
    pub bytes: u64,
    pub has_camera: bool,
    pub modified_s: u64,
}

fn folder_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => stack.push(e.path()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

pub fn summarize(dir: &Path) -> Option<ProjectSummary> {
    let p = ProjectV1::load(dir).ok()?;
    let modified_s = std::fs::metadata(dir.join(ProjectV1::FILE_NAME))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Some(ProjectSummary {
        dir: dir.to_path_buf(),
        title: p.title.clone(),
        created_at: p.created_at.clone(),
        state: p.state,
        duration_ns: p.duration_ns(),
        edited_ns: sc_player::timeline_doc(&p).duration(),
        bytes: folder_bytes(dir),
        has_camera: p
            .recording
            .tracks
            .iter()
            .any(|t| t.kind == sc_core::project::TrackKind::Camera),
        modified_s,
    })
}

/// Every project folder directly inside `root`, newest first.
pub fn list(root: &Path) -> Vec<ProjectSummary> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<ProjectSummary> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| sc_core::folder::is_project_dir(p))
        .filter_map(|p| summarize(&p))
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.dir.cmp(&a.dir)));
    out
}

/// The newest project that can be edited (not still recording).
pub fn latest_editable(root: &Path) -> Option<PathBuf> {
    list(root)
        .into_iter()
        .find(|s| s.state != ProjectState::Recording)
        .map(|s| s.dir)
}

/// A small picture of the composed video (screen, camera and background) at one second or a
/// third of the way in, cached in the project's thumbs folder.
pub fn poster(
    dir: &Path,
    decoder: std::sync::Arc<dyn sc_platform::Decoder>,
) -> Result<std::path::PathBuf, String> {
    let file = dir.join("thumbs").join("poster-v1.png");
    if file.is_file() {
        return Ok(file);
    }
    let project = ProjectV1::load(dir).map_err(|e| e.to_string())?;
    let (_, ratio) = sc_export::plan::reference(&project);
    let w = 480u32;
    let h = ((f64::from(w) / ratio.as_f64()).round() as u32).max(2) & !1;
    let mut r = sc_export::recipe::FrameRenderer::new(
        &project,
        dir,
        decoder,
        sc_geometry::SizePx { w, h },
        sc_render::Backend::Auto,
        sc_render::OutputFormat::Rgba8,
    )
    .map_err(|e| e.to_string())?;
    let len = sc_player::timeline_doc(&project).duration();
    let at = (len / 3).clamp(0, 1_000_000_000);
    let frame = r.render(at).map_err(|e| e.to_string())?.0;
    let png = sc_text::encode_png(&sc_core::image::RgbaImage {
        size: frame.size,
        stride: frame.size.w as usize * 4,
        data: frame.data,
    })
    .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("thumbs")).map_err(|e| e.to_string())?;
    sc_core::fsio::atomic_write(&file, &png).map_err(|e| e.to_string())?;
    Ok(file)
}

/// `name` names a project folder directly inside `root` (no separators, no `..`).
pub fn project_in(root: &Path, name: &str) -> Option<std::path::PathBuf> {
    let ok = !name.is_empty() && !name.contains(['/', '\\']) && name != ".." && name != ".";
    let dir = root.join(name);
    (ok && sc_core::folder::is_project_dir(&dir)).then_some(dir)
}
