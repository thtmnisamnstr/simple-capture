//! The App Sandbox the macOS app runs in. Export folders the person picked stay reachable after a
//! relaunch through security-scoped bookmarks kept in export.json; exports to them are staged in
//! the project folder, because the FFmpeg helper's inherited sandbox reaches ~/Movies (a static
//! entitlement) but not a folder granted at run time; and a picked background image is copied
//! into the container, since the recorder keeps its path.

use std::path::{Component, Path, PathBuf};

use sc_capture_macos::workspace;
use sc_export::settings::ExportSettings;

use crate::exports::{load, save};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len())
        .step_by(2)
        .map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
        .collect()
}

fn in_movies(path: &Path) -> bool {
    !path.components().any(|c| c == Component::ParentDir)
        && workspace::real_home().is_ok_and(|h| path.starts_with(h.join("Movies")))
}

/// Keeps a bookmark of the folder just picked in the folder panel. Bookmarks of folders no
/// remembered setting or preset uses any more are dropped.
pub fn remember_export_folder(data: &Path, folder: &Path) {
    let b = match workspace::bookmark(folder) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(folder = %folder.display(), "export folder bookmark: {e}");
            return;
        }
    };
    let mut store = load(data);
    let used: Vec<PathBuf> = store
        .per_format
        .values()
        .chain(store.presets.iter().map(|p| &p.settings))
        .filter_map(|s| s.out_dir.clone())
        .collect();
    store
        .folder_bookmarks
        .retain(|k, _| used.iter().any(|u| u.as_path() == Path::new(k)));
    store
        .folder_bookmarks
        .insert(folder.to_string_lossy().into_owned(), hex(&b));
    if let Err(e) = save(data, &store) {
        tracing::warn!("export folder bookmark: {e}");
    }
}

/// Opens to this process, until it quits, the picked folder that holds `path`, and renews its
/// bookmark when macOS reports it stale.
pub fn reach_export_folder(data: &Path, path: &Path) -> Result<(), String> {
    let mut store = load(data);
    let (key, hexed) = store
        .folder_bookmarks
        .iter()
        .filter(|(k, _)| path.starts_with(k))
        .max_by_key(|(k, _)| k.len())
        .map(|(k, v)| (k.clone(), v.clone()))
        .ok_or("no bookmark")?;
    let bytes = unhex(&hexed).ok_or("a damaged bookmark")?;
    let (now, stale) = workspace::start_access(&bytes).map_err(|e| e.to_string())?;
    if now.as_path() != Path::new(&key) {
        return Err(format!("the folder moved to {}", now.display()));
    }
    if stale {
        match workspace::bookmark(&now) {
            Ok(b) => {
                store.folder_bookmarks.insert(key, hex(&b));
                if let Err(e) = save(data, &store) {
                    tracing::warn!(folder = %path.display(), "renew bookmark: {e}");
                }
            }
            Err(e) => tracing::warn!(folder = %now.display(), "renew bookmark: {e}"),
        }
    }
    Ok(())
}

/// Where the export helper writes before the file moves into the settings' folder: the
/// project folder, for a folder outside ~/Movies.
pub fn export_stage(
    data: &Path,
    settings: &ExportSettings,
    project_dir: &Path,
) -> Result<Option<PathBuf>, String> {
    let Some(out) = settings.out_dir.as_deref() else {
        return Ok(None);
    };
    if in_movies(out) {
        return Ok(None);
    }
    reach_export_folder(data, out).map_err(|e| {
        tracing::warn!(folder = %out.display(), "export folder: {e}");
        format!(
            "simple-capture can no longer save to “{}”. Choose the folder again.",
            out.display()
        )
    })?;
    Ok(Some(project_dir.to_path_buf()))
}

/// A copy of the picked image in `<data>/backgrounds`, or `src` itself when it can't be copied.
/// Earlier copies go, except the one the recorder settings use (`keep`).
pub fn keep_background_image(data: &Path, src: &Path, keep: Option<&str>) -> PathBuf {
    let root = data.join("backgrounds");
    let copy = || -> std::io::Result<PathBuf> {
        let name = src.file_name().ok_or(std::io::ErrorKind::InvalidInput)?;
        if std::fs::metadata(src)?.len() > sc_core::project::MAX_BACKGROUND_BYTES {
            return Err(std::io::ErrorKind::FileTooLarge.into());
        }
        for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            let d = e.path();
            if keep.is_none_or(|k| !Path::new(k).starts_with(&d)) {
                let _ = std::fs::remove_dir_all(&d);
            }
        }
        let dir = root.join(sc_core::ids::new_id());
        std::fs::create_dir_all(&dir)?;
        let to = dir.join(name);
        std::fs::copy(src, &to)?;
        Ok(to)
    };
    copy().unwrap_or_else(|e| {
        tracing::warn!(image = %src.display(), "keep a background copy: {e}");
        src.to_path_buf()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let b = [0u8, 1, 0xab, 0xff];
        assert_eq!(hex(&b), "0001abff");
        assert_eq!(unhex("0001abff").as_deref(), Some(&b[..]));
        assert_eq!(unhex("0g"), None);
        assert_eq!(unhex("abc"), None);
    }

    #[test]
    fn movies_is_the_real_one() {
        let home = workspace::real_home().expect("home");
        assert!(in_movies(&home.join("Movies/simple-capture")));
        assert!(!in_movies(&home.join("Desktop")));
        assert!(!in_movies(&home.join("Movies/../Desktop")));
    }
}
