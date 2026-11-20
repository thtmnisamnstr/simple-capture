//! Project folder naming and locking (ADR-032; Q4: plain folder with the `.sc` suffix).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::project::ProjectV1;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalDateTime {
    pub y: i32,
    pub mo: u8,
    pub d: u8,
    pub h: u8,
    pub mi: u8,
    pub s: u8,
    pub offset_min: i32,
}

impl LocalDateTime {
    /// The wall clock now, in the local time zone.
    pub fn now() -> LocalDateTime {
        local_now()
    }

    /// The UTC civil time of `secs` seconds since the Unix epoch, shifted by `offset_min`.
    pub fn from_unix(secs: i64, offset_min: i32) -> LocalDateTime {
        let local = secs + i64::from(offset_min) * 60;
        let days = local.div_euclid(86_400);
        let rem = local.rem_euclid(86_400);
        let (y, mo, d) = civil_from_days(days);
        LocalDateTime {
            y,
            mo,
            d,
            h: (rem / 3600) as u8,
            mi: (rem % 3600 / 60) as u8,
            s: (rem % 60) as u8,
            offset_min,
        }
    }
}

pub const PROJECT_SUFFIX: &str = ".sc";

/// Longest title kept in a folder name, in UTF-8 bytes. With the date prefix, a collision
/// suffix and `.sc` the name stays far below the 255-byte limit of APFS, NTFS and ext4, and
/// Windows paths stay short.
pub const MAX_TITLE_BYTES: usize = 100;

const DEFAULT_TITLE: &str = "Recording";

/// `"<YYYY-MM-DD HH.MM.SS> <title>.sc"` with the title sanitized for macOS, Windows and Linux.
pub fn project_folder_name(t: &LocalDateTime, title: &str) -> String {
    folder_name_n(t, title, 1)
}

fn folder_name_n(t: &LocalDateTime, title: &str, n: u32) -> String {
    let collision = if n > 1 {
        format!(" {n}")
    } else {
        String::new()
    };
    format!(
        "{:04}-{:02}-{:02} {:02}.{:02}.{:02} {}{collision}{PROJECT_SUFFIX}",
        t.y,
        t.mo,
        t.d,
        t.h,
        t.mi,
        t.s,
        sanitize_title(title)
    )
}

/// Replaces characters that are invalid in a file name on any of the three OSes (and control
/// characters) with `-`, collapses whitespace, trims, and caps the length at a char boundary.
pub fn sanitize_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut last_space = true;
    for c in title.chars() {
        let c = match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => ' ',
            c => c,
        };
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    if out.len() > MAX_TITLE_BYTES {
        let mut cut = MAX_TITLE_BYTES;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        // Do not leave a dangling zero-width joiner or variation selector from a split emoji.
        while out.ends_with(['\u{200D}', '\u{FE0F}', '\u{FE0E}']) {
            out.pop();
        }
    }
    let trimmed = out.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if trimmed.is_empty() {
        DEFAULT_TITLE.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Creates `<root>/<name>` (creating `root` if needed). When the name is taken, tries
/// `"… <title> 2.sc"`, `" 3"`, … Never reuses an existing folder.
pub fn create_project_dir(root: &Path, t: &LocalDateTime, title: &str) -> Result<PathBuf> {
    fs::create_dir_all(root).map_err(|e| Error::io_at(e, "create folder", root))?;
    for n in 1..=999 {
        let dir = root.join(folder_name_n(t, title, n));
        match fs::create_dir(&dir) {
            Ok(()) => {
                crate::fsio::fsync_dir(root)?;
                return Ok(dir);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::io_at(e, "create project folder", &dir)),
        }
    }
    Err(Error::Other(format!(
        "no free project folder name for {:?} in {}",
        project_folder_name(t, title),
        root.display()
    )))
}

/// A folder whose name ends in `.sc` and that holds a `project.json` or a `journal.jsonl` (a
/// recording that crashed before its first `project.json` still counts).
pub fn is_project_dir(p: &Path) -> bool {
    let named = p
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.len() > PROJECT_SUFFIX.len() && n.ends_with(PROJECT_SUFFIX));
    named
        && p.is_dir()
        && (p.join(ProjectV1::FILE_NAME).is_file() || p.join(crate::journal::FILE_NAME).is_file())
}

pub const LOCK_FILE_NAME: &str = ".lock";

/// A lock file whose content cannot be read as a pid is treated as being written right now
/// until it is this old.
const UNREADABLE_LOCK_GRACE: Duration = Duration::from_secs(10);

/// Per-project `.lock`; `Err(Locked)` when another live process holds it.
///
/// The lock is an exclusively created `.lock` file holding `"<pid> <token>"`. On unix a lock whose
/// pid is dead is stale and taken over; on Windows an existing lock is always treated as held.
/// Dropping the lock removes the file if it is still ours.
#[derive(Debug)]
pub struct ProjectLock {
    path: PathBuf,
    content: String,
}

impl ProjectLock {
    pub fn acquire(dir: &Path) -> Result<ProjectLock> {
        let path = dir.join(LOCK_FILE_NAME);
        let content = format!("{} {}\n", std::process::id(), crate::ids::new_id());
        for _ in 0..3 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    f.write_all(content.as_bytes())
                        .and_then(|()| f.sync_all())
                        .map_err(|e| Error::io_at(e, "write lock", &path))?;
                    return Ok(ProjectLock { path, content });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if !lock_is_stale(&path) {
                        return Err(Error::Locked(path));
                    }
                    match fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(Error::io_at(e, "remove stale lock", &path)),
                    }
                }
                Err(e) => return Err(Error::io_at(e, "create lock", &path)),
            }
        }
        Err(Error::Locked(path))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        if fs::read_to_string(&self.path).is_ok_and(|c| c == self.content) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn lock_is_stale(path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        // Vanished between create and read: let the caller retry the create.
        return !path.exists();
    };
    match content
        .split_whitespace()
        .next()
        .and_then(|p| p.parse::<u32>().ok())
    {
        Some(pid) => !pid_alive(pid),
        None => fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok())
            .is_some_and(|age| age > UNREADABLE_LOCK_GRACE),
    }
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return true;
    };
    if pid <= 0 {
        return true;
    }
    // SAFETY: signal 0 only checks whether the process exists; no signal is sent.
    let rc = unsafe { libc::kill(pid, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool {
    true
}

/// `YYYY-MM-DDTHH:MM:SS±hh:mm`.
pub fn rfc3339(t: &LocalDateTime) -> String {
    let sign = if t.offset_min < 0 { '-' } else { '+' };
    let off = t.offset_min.unsigned_abs();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
        t.y,
        t.mo,
        t.d,
        t.h,
        t.mi,
        t.s,
        off / 60,
        off % 60
    )
}

/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(z: i64) -> (i32, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y as i32, m, d)
}

#[cfg(windows)]
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn unix_now_secs() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(unix)]
fn local_now() -> LocalDateTime {
    let secs = unix_now_secs();
    let t = secs as libc::time_t;
    // SAFETY: an all-zero `tm` is a valid value; localtime_r writes into it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the duration of the call.
    let r = unsafe { libc::localtime_r(&t, &mut tm) };
    if r.is_null() {
        return LocalDateTime::from_unix(secs, 0);
    }
    LocalDateTime::from_unix(secs, (tm.tm_gmtoff / 60) as i32)
}

#[cfg(windows)]
fn local_now() -> LocalDateTime {
    let (l, u) = crate::time::win::local_and_utc();
    let secs_of = |st: &crate::time::win::SystemTime| {
        days_from_civil(i64::from(st.year), i64::from(st.month), i64::from(st.day)) * 86_400
            + i64::from(st.hour) * 3600
            + i64::from(st.minute) * 60
            + i64::from(st.second)
    };
    let diff = secs_of(&l) - secs_of(&u);
    // Round to 15 minutes: the two clock reads may straddle a second boundary.
    let offset_min = ((diff as f64 / 900.0).round() as i32) * 15;
    LocalDateTime::from_unix(unix_now_secs(), offset_min)
}

#[cfg(not(any(unix, windows)))]
fn local_now() -> LocalDateTime {
    LocalDateTime::from_unix(unix_now_secs(), 0)
}
