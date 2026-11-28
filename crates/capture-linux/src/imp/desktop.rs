use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};
use ashpd::desktop::open_uri::OpenDirectoryRequest;
use sc_platform::{Desktop, Error, Result};

use super::portal::runtime;

fn has(cmd: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {cmd}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn uri(path: &Path) -> String {
    let mut s = String::from("file://");
    for b in path.as_os_str().as_encoded_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                s.push(*b as char);
            }
            other => s.push_str(&format!("%{other:02X}")),
        }
    }
    s
}

fn file_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

fn choose(folders: bool, done: Box<dyn FnOnce(Option<PathBuf>) + Send>) {
    runtime().spawn(async move {
        let parent = tokio::task::spawn_blocking(super::parent::focused)
            .await
            .ok()
            .flatten();
        let mut req = SelectedFiles::open_file()
            .identifier(parent)
            .title(if folders {
                "Choose a Folder"
            } else {
                "Choose an Image"
            })
            .modal(true)
            .multiple(false)
            .directory(folders);
        if !folders {
            req = req.filter(
                FileFilter::new("Images")
                    .mimetype("image/png")
                    .mimetype("image/jpeg"),
            );
        }
        let path = match req.send().await.and_then(|r| r.response()) {
            Ok(files) => files.uris().first().and_then(|u| file_path(u.as_str())),
            Err(_) => None,
        };
        done(path);
    });
}

/// The freedesktop.org Trash spec for the home Trash: the item moves to `files/`, and an
/// `info/<name>.trashinfo` records where it came from. Only within one filesystem.
fn xdg_trash(path: &Path) -> Result<()> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or(Error::NotFound("home folder".into()))?;
    xdg_trash_in(&data, path)
}

fn xdg_trash_in(data: &Path, path: &Path) -> Result<()> {
    let trash = data.join("Trash");
    std::fs::create_dir_all(trash.join("files"))?;
    std::fs::create_dir_all(trash.join("info"))?;
    let base = path
        .file_name()
        .ok_or(Error::Other("nothing to trash".into()))?
        .to_string_lossy()
        .into_owned();
    let mut name = base.clone();
    let mut n = 2;
    while trash.join("files").join(&name).exists()
        || trash
            .join("info")
            .join(format!("{name}.trashinfo"))
            .exists()
    {
        name = format!("{base} {n}");
        n += 1;
    }
    let abs = std::fs::canonicalize(path)?;
    let t = super::sys::local_now();
    let info = format!(
        "[Trash Info]\nPath={}\nDeletionDate={:04}-{:02}-{:02}T{:02}:{:02}:{:02}\n",
        uri(&abs).trim_start_matches("file://"),
        t.y,
        t.mo,
        t.d,
        t.h,
        t.mi,
        t.s
    );
    std::fs::write(trash.join("info").join(format!("{name}.trashinfo")), info)?;
    std::fs::rename(&abs, trash.join("files").join(&name)).map_err(|e| {
        let _ = std::fs::remove_file(trash.join("info").join(format!("{name}.trashinfo")));
        Error::Other(format!("move to the Trash: {e}"))
    })
}

/// Puts `path` on the clipboard as a file, through the app's own GTK on its main thread, so it
/// needs no wl-copy or xclip (which sandboxes and many desktops lack). False when GTK refused.
fn gtk_copy_file(path: &Path) -> bool {
    use gtk::{TargetEntry, TargetFlags};
    let uri = uri(path);
    let text = path.to_string_lossy().into_owned();
    let (tx, rx) = std::sync::mpsc::channel();
    gtk::glib::MainContext::default().invoke(move || {
        let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
        let targets = [
            TargetEntry::new("text/uri-list", TargetFlags::empty(), 0),
            TargetEntry::new("x-special/gnome-copied-files", TargetFlags::empty(), 1),
            TargetEntry::new("UTF8_STRING", TargetFlags::empty(), 2),
            TargetEntry::new("text/plain;charset=utf-8", TargetFlags::empty(), 2),
        ];
        let ok = clipboard.set_with_data(&targets, move |_, sel, info| {
            let data = match info {
                0 => format!("{uri}\r\n"),
                1 => format!("copy\n{uri}"),
                _ => text.clone(),
            };
            sel.set(&sel.target(), 8, data.as_bytes());
        });
        let _ = tx.send(ok);
    });
    rx.recv_timeout(std::time::Duration::from_secs(2))
        .unwrap_or(false)
}

pub struct LinuxDesktop;

impl Desktop for LinuxDesktop {
    fn reveal(&self, path: &Path) -> Result<()> {
        let opened = std::fs::File::open(path)
            .ok()
            .map(|f| runtime().block_on(async { OpenDirectoryRequest::default().send(&f).await }));
        if matches!(opened, Some(Ok(_))) {
            return Ok(());
        }
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        Command::new("xdg-open").arg(dir).spawn()?;
        Ok(())
    }

    fn choose_image(&self, done: Box<dyn FnOnce(Option<PathBuf>) + Send>) {
        choose(false, done);
    }

    fn choose_folder(&self, done: Box<dyn FnOnce(Option<PathBuf>) + Send>) {
        choose(true, done);
    }

    fn trash(&self, path: &Path) -> Result<()> {
        if has("gio")
            && Command::new("gio")
                .args(["trash", "--"])
                .arg(path)
                .status()
                .is_ok_and(|s| s.success())
        {
            return Ok(());
        }
        xdg_trash(path)
    }

    fn copy_file(&self, path: &Path) -> Result<()> {
        if gtk_copy_file(path) {
            return Ok(());
        }
        let list = format!("{}\r\n", uri(path));
        let mut cmd = if std::env::var_os("WAYLAND_DISPLAY").is_some() && has("wl-copy") {
            let mut c = Command::new("wl-copy");
            c.args(["--type", "text/uri-list"]);
            c
        } else if has("xclip") {
            let mut c = Command::new("xclip");
            c.args(["-selection", "clipboard", "-t", "text/uri-list"]);
            c
        } else {
            return Err(Error::Unsupported(
                "copying a file needs wl-copy (Wayland) or xclip (X11)",
            ));
        };
        let mut child = cmd.stdin(Stdio::piped()).spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin.write_all(list.as_bytes())?;
        }
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freedesktop_trash_moves_the_folder_and_records_where_it_was() {
        let root = std::env::temp_dir().join(format!("sc-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let item = root.join("Recording one.sc");
        std::fs::create_dir_all(item.join("tracks")).expect("item");
        std::fs::write(item.join("project.json"), "{}").expect("file");
        let data = root.join("share");
        xdg_trash_in(&data, &item).expect("trash");
        assert!(!item.exists());
        assert!(
            data.join("Trash/files/Recording one.sc/project.json")
                .exists()
        );
        let info = std::fs::read_to_string(data.join("Trash/info/Recording one.sc.trashinfo"))
            .expect("info");
        assert!(info.starts_with("[Trash Info]\nPath=/"));
        assert!(info.contains("Recording%20one.sc"));
        std::fs::create_dir_all(&item).expect("again");
        xdg_trash_in(&data, &item).expect("second");
        assert!(data.join("Trash/files/Recording one.sc 2").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
