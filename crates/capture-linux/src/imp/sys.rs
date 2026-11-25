use std::ffi::CString;
use std::path::{Path, PathBuf};

use sc_platform::{Error, HostTime, LocalDateTime, Result};

pub fn host_now() -> HostTime {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    HostTime(ts.tv_sec * 1_000_000_000 + ts.tv_nsec)
}

pub fn local_now() -> LocalDateTime {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    LocalDateTime {
        y: tm.tm_year + 1900,
        mo: (tm.tm_mon + 1) as u8,
        d: tm.tm_mday as u8,
        h: tm.tm_hour as u8,
        mi: tm.tm_min as u8,
        s: tm.tm_sec as u8,
        offset_min: (tm.tm_gmtoff / 60) as i32,
    }
}

pub fn free_disk_bytes(path: &Path) -> Result<u64> {
    let mut p = path;
    while !p.exists() {
        p = p
            .parent()
            .ok_or_else(|| Error::NotFound(path.display().to_string()))?;
    }
    let c =
        CString::new(p.as_os_str().as_encoded_bytes()).map_err(|e| Error::Other(e.to_string()))?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(st.f_bavail as u64 * st.f_frsize as u64)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

/// `XDG_VIDEOS_DIR` from user-dirs.dirs, else ~/Videos.
pub fn videos_dir() -> PathBuf {
    let config =
        std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home().join(".config"), PathBuf::from);
    let from_file = std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|s| {
            s.lines()
                .filter_map(|l| l.trim().strip_prefix("XDG_VIDEOS_DIR="))
                .map(|v| {
                    v.trim_matches('"')
                        .replace("$HOME", &home().to_string_lossy())
                })
                .next()
        })
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p != &home());
    from_file.unwrap_or_else(|| home().join("Videos"))
}
