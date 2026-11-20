//! CPU% comes from cumulative CPU-time deltas (`ps -o time`), not from ps's
//! decaying %cpu column, which lags and smooths over the measurement window.

use serde::Serialize;
use std::process::Command;

#[derive(Clone, Debug, Serialize)]
pub struct ProcSample {
    pub pid: i32,
    pub role: String,
    pub cpu_s: f64,
    pub rss_mb: f64,
    pub footprint_mb: Option<f64>,
}

fn parse_clock(s: &str) -> Option<f64> {
    let s = s.trim();
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<f64>().ok()?, r),
        None => (0.0, s),
    };
    let mut total = 0.0;
    for part in rest.split(':') {
        total = total * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(days * 86400.0 + total)
}

fn role_of(comm: &str) -> Option<&'static str> {
    if comm.contains("com.apple.WebKit.WebContent") || comm.contains("WebKitWebProcess") {
        Some("webkit-webcontent")
    } else if comm.contains("com.apple.WebKit.GPU") || comm.contains("WebKitGPUProcess") {
        Some("webkit-gpu")
    } else if comm.contains("com.apple.WebKit.Networking") || comm.contains("WebKitNetworkProcess") {
        Some("webkit-networking")
    } else if comm.contains("com.apple.WebKit") {
        Some("webkit-other")
    } else {
        None
    }
}

// WebKit XPC services are spawned by launchd, not as our children, so any
// WebKit process younger than this app is attributed to it.
pub fn discover(app_uptime_s: f64) -> Vec<(i32, String)> {
    let mut out = vec![(std::process::id() as i32, "app".to_string())];
    let Ok(o) = Command::new("ps").args(["-axo", "pid=,etime=,comm="]).output() else {
        return out;
    };
    let text = String::from_utf8_lossy(&o.stdout);
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(pid), Some(etime)) = (it.next(), it.next()) else { continue };
        let comm: String = it.collect::<Vec<_>>().join(" ");
        let Some(role) = role_of(&comm) else { continue };
        let (Ok(pid), Some(age)) = (pid.parse::<i32>(), parse_clock(etime)) else { continue };
        if age <= app_uptime_s + 1.0 {
            out.push((pid, role.to_string()));
        }
    }
    if let Some(ws) = text.lines().find(|l| l.trim_end().ends_with("/WindowServer")) {
        if let Some(pid) = ws.split_whitespace().next().and_then(|p| p.parse().ok()) {
            out.push((pid, "windowserver(system-wide)".to_string()));
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn footprint_mb(pid: i32) -> Option<f64> {
    unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        let rc = libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V2,
            &mut info as *mut _ as *mut libc::rusage_info_t,
        );
        if rc == 0 {
            Some(info.ri_phys_footprint as f64 / 1048576.0)
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn footprint_mb(_pid: i32) -> Option<f64> {
    None
}

pub fn sample(procs: &[(i32, String)]) -> Vec<ProcSample> {
    let list = procs.iter().map(|(p, _)| p.to_string()).collect::<Vec<_>>().join(",");
    let Ok(o) = Command::new("ps").args(["-o", "pid=,time=,rss=", "-p", &list]).output() else {
        return vec![];
    };
    let text = String::from_utf8_lossy(&o.stdout);
    let mut out = vec![];
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 {
            continue;
        }
        let Ok(pid) = f[0].parse::<i32>() else { continue };
        let role = procs.iter().find(|(p, _)| *p == pid).map(|(_, r)| r.clone()).unwrap_or_default();
        out.push(ProcSample {
            pid,
            role,
            cpu_s: parse_clock(f[1]).unwrap_or(f64::NAN),
            rss_mb: f[2].parse::<f64>().unwrap_or(0.0) / 1024.0,
            footprint_mb: footprint_mb(pid),
        });
    }
    out
}

pub fn gpu_utilization() -> Option<f64> {
    let o = Command::new("ioreg").args(["-r", "-d", "1", "-w", "0", "-c", "IOAccelerator"]).output().ok()?;
    let text = String::from_utf8_lossy(&o.stdout);
    let key = "\"Device Utilization %\"=";
    let i = text.find(key)? + key.len();
    let num: String = text[i..].chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse().ok()
}
