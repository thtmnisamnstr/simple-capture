use std::path::{Path, PathBuf};

use sc_platform::{Error, HostTime, LocalDateTime, PermissionKind, PermissionStatus, Result};
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
use windows::Win32::System::Time::SystemTimeToFileTime;
use windows::Win32::UI::Shell::{FOLDERID_Videos, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};
use windows::core::{HSTRING, PCWSTR};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn host_now() -> HostTime {
    let (mut c, mut f) = (0i64, 0i64);
    unsafe {
        let _ = QueryPerformanceCounter(&mut c);
        let _ = QueryPerformanceFrequency(&mut f);
    }
    HostTime((i128::from(c) * 1_000_000_000 / i128::from(f.max(1))) as i64)
}

fn filetime_100ns(t: &SYSTEMTIME) -> i64 {
    let mut ft = FILETIME::default();
    unsafe {
        let _ = SystemTimeToFileTime(t, &mut ft);
    }
    (i64::from(ft.dwHighDateTime) << 32) | i64::from(ft.dwLowDateTime)
}

pub fn local_now() -> LocalDateTime {
    let (local, utc) = unsafe { (GetLocalTime(), GetSystemTime()) };
    let offset_min =
        ((filetime_100ns(&local) - filetime_100ns(&utc)) as f64 / 600_000_000.0).round() as i32;
    LocalDateTime {
        y: i32::from(local.wYear),
        mo: local.wMonth as u8,
        d: local.wDay as u8,
        h: local.wHour as u8,
        mi: local.wMinute as u8,
        s: local.wSecond as u8,
        offset_min,
    }
}

pub fn free_disk_bytes(path: &Path) -> Result<u64> {
    let mut p = path;
    while !p.exists() {
        p = p
            .parent()
            .ok_or_else(|| Error::NotFound(path.display().to_string()))?;
    }
    let mut avail = 0u64;
    unsafe {
        GetDiskFreeSpaceExW(&HSTRING::from(p.as_os_str()), Some(&mut avail), None, None)
            .map_err(|e| Error::Other(format!("free space of {}: {e}", p.display())))?;
    }
    Ok(avail)
}

pub fn default_projects_dir() -> Result<PathBuf> {
    let videos = unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Videos, KNOWN_FOLDER_FLAG(0), None)
            .map_err(|e| Error::Other(format!("Videos folder: {e}")))?;
        let s = p.to_string();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map_err(|e| Error::Other(format!("Videos folder: {e}")))?
    };
    Ok(PathBuf::from(videos).join("Simple Capture"))
}

fn consent(key: &str) -> Option<String> {
    let k = wide(key);
    let v = wide("Value");
    let mut buf = [0u16; 64];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(v.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if r.is_err() {
        return None;
    }
    let n = (len as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]))
}

pub fn permission(k: PermissionKind) -> PermissionStatus {
    let store = match k {
        PermissionKind::Screen => return PermissionStatus::Granted,
        PermissionKind::Camera => "webcam",
        PermissionKind::Microphone => "microphone",
    };
    let base = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\{store}"
    );
    let denied = [consent(&base), consent(&format!("{base}\\NonPackaged"))]
        .iter()
        .flatten()
        .any(|v| v.eq_ignore_ascii_case("Deny"));
    if denied {
        PermissionStatus::Denied
    } else {
        PermissionStatus::Granted
    }
}

pub fn settings_url(k: PermissionKind) -> Option<String> {
    Some(
        match k {
            PermissionKind::Screen => "ms-settings:privacy",
            PermissionKind::Camera => "ms-settings:privacy-webcam",
            PermissionKind::Microphone => "ms-settings:privacy-microphone",
        }
        .to_string(),
    )
}
