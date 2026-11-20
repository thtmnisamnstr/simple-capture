use std::sync::mpsc;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::CFString;
use objc2_core_media::{CMClock, CMTime, CMTimeFlags};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSDictionary, NSNumber, NSRunLoop, NSString};

#[allow(deprecated)]
pub fn timebase() -> (u32, u32) {
    let mut tb = libc::mach_timebase_info { numer: 0, denom: 0 };
    unsafe { libc::mach_timebase_info(&mut tb) };
    (tb.numer, tb.denom)
}

#[allow(deprecated)]
pub fn mach_now() -> u64 {
    unsafe { libc::mach_absolute_time() }
}

pub fn mach_to_secs(t: u64) -> f64 {
    let (n, d) = timebase();
    (t as f64) * (n as f64) / (d as f64) / 1e9
}

pub fn host_clock_now() -> CMTime {
    unsafe { CMClock::host_time_clock().time() }
}

pub fn host_now_secs() -> f64 {
    secs(host_clock_now())
}

pub fn secs(t: CMTime) -> f64 {
    unsafe { t.seconds() }
}

pub fn cmtime_secs(s: f64) -> CMTime {
    unsafe { CMTime::with_seconds(s, 1_000_000_000) }
}

pub fn cmtime_valid(t: &CMTime) -> bool {
    t.flags.contains(CMTimeFlags::Valid)
}

pub fn nsstr(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

pub fn cfstr_as_ns(s: &CFString) -> &NSString {
    unsafe { &*(s as *const CFString as *const NSString) }
}

pub fn dict(pairs: &[(&NSString, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<&NSString> = pairs.iter().map(|p| p.0).collect();
    let vals: Vec<&AnyObject> = pairs.iter().map(|p| p.1).collect();
    NSDictionary::from_slices(&keys, &vals)
}

pub fn num_i(v: i64) -> Retained<NSNumber> {
    NSNumber::new_i64(v)
}

pub fn num_f(v: f64) -> Retained<NSNumber> {
    NSNumber::new_f64(v)
}

pub fn num_b(v: bool) -> Retained<NSNumber> {
    NSNumber::new_bool(v)
}

pub fn as_obj<T: objc2::Message>(r: &T) -> &AnyObject {
    unsafe { &*(r as *const T as *const AnyObject) }
}

/// Pump the current thread's run loop until `rx` yields or the timeout elapses.
pub fn pump_recv<T>(rx: &mpsc::Receiver<T>, timeout: Duration) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(v) = rx.try_recv() {
            return Some(v);
        }
        if Instant::now() >= deadline {
            return None;
        }
        pump(Duration::from_millis(5));
    }
}

pub fn pump(d: Duration) {
    unsafe {
        let date = NSDate::dateWithTimeIntervalSinceNow(d.as_secs_f64());
        NSRunLoop::currentRunLoop().runMode_beforeDate(NSDefaultRunLoopMode, &date);
    }
}

pub fn pump_for(d: Duration) {
    let deadline = Instant::now() + d;
    while Instant::now() < deadline {
        pump(Duration::from_millis(4));
    }
}

pub fn fourcc(v: u32) -> String {
    let b = v.to_be_bytes();
    b.iter().map(|c| if c.is_ascii_graphic() { *c as char } else { '?' }).collect()
}

pub fn write_json(path: &std::path::Path, v: &serde_json::Value) -> anyhow::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(v)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcCpu {
    pub pid: i32,
    pub cpu_secs: f64,
}

const PROC_PIDTASKINFO: libc::c_int = 4;

#[repr(C)]
#[derive(Default)]
struct ProcTaskInfo {
    pti_virtual_size: u64,
    pti_resident_size: u64,
    pti_total_user: u64,
    pti_total_system: u64,
    pti_threads_user: u64,
    pti_threads_system: u64,
    pti_policy: i32,
    pti_faults: i32,
    pti_pageins: i32,
    pti_cow_faults: i32,
    pti_messages_sent: i32,
    pti_messages_received: i32,
    pti_syscalls_mach: i32,
    pti_syscalls_unix: i32,
    pti_csw: i32,
    pti_threadnum: i32,
    pti_numrunning: i32,
    pti_priority: i32,
}

extern "C" {
    fn proc_pidinfo(pid: libc::c_int, flavor: libc::c_int, arg: u64, buffer: *mut libc::c_void, size: libc::c_int) -> libc::c_int;
    fn proc_listallpids(buffer: *mut libc::c_void, size: libc::c_int) -> libc::c_int;
    fn proc_name(pid: libc::c_int, buffer: *mut libc::c_void, size: u32) -> libc::c_int;
}

/// Returns (cpu seconds, resident bytes) for a pid, or None when it is not readable.
pub fn proc_cpu(pid: i32) -> Option<(f64, u64)> {
    let mut info = ProcTaskInfo::default();
    let sz = std::mem::size_of::<ProcTaskInfo>() as libc::c_int;
    let r = unsafe { proc_pidinfo(pid, PROC_PIDTASKINFO, 0, &mut info as *mut _ as *mut libc::c_void, sz) };
    if r != sz {
        return None;
    }
    let (n, d) = timebase();
    let ticks = info.pti_total_user + info.pti_total_system;
    Some(((ticks as f64) * (n as f64) / (d as f64) / 1e9, info.pti_resident_size))
}

pub fn pids_named(name: &str) -> Vec<i32> {
    let mut buf = vec![0i32; 8192];
    let n = unsafe { proc_listallpids(buf.as_mut_ptr() as *mut libc::c_void, (buf.len() * 4) as libc::c_int) };
    let mut out = vec![];
    for &pid in buf.iter().take(n.max(0) as usize) {
        if pid <= 0 {
            continue;
        }
        let mut nb = [0u8; 256];
        let l = unsafe { proc_name(pid, nb.as_mut_ptr() as *mut libc::c_void, 256) };
        if l > 0 && &nb[..l as usize] == name.as_bytes() {
            out.push(pid);
        }
    }
    out
}

/// Sum of CPU seconds of all processes with these names (helper daemons do the
/// capture and encode work outside our process).
pub fn cpu_snapshot(names: &[&str]) -> Vec<(String, f64)> {
    let mut out = vec![];
    out.push(("self".to_string(), proc_cpu(std::process::id() as i32).map(|v| v.0).unwrap_or(0.0)));
    for n in names {
        let total: f64 = pids_named(n).iter().filter_map(|p| proc_cpu(*p)).map(|v| v.0).sum();
        out.push((n.to_string(), total));
    }
    out
}

pub fn self_rss() -> u64 {
    proc_cpu(std::process::id() as i32).map(|v| v.1).unwrap_or(0)
}
