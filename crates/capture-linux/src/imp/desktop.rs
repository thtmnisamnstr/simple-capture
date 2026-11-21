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
        let mut req = SelectedFiles::open_file()
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

    fn copy_file(&self, path: &Path) -> Result<()> {
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
