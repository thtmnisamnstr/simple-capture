//! Process CPU and RSS, ported from spike 3's `util.rs` (`proc_pidinfo` needs no privilege for
//! the user's own processes; replayd and VTEncoderXPCService run as the user).

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcInfo {
    pub cpu_s: f64,
    pub rss_bytes: u64,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::ProcInfo;

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

    unsafe extern "C" {
        fn proc_pidinfo(
            pid: libc::c_int,
            flavor: libc::c_int,
            arg: u64,
            buffer: *mut libc::c_void,
            size: libc::c_int,
        ) -> libc::c_int;
        fn proc_listallpids(buffer: *mut libc::c_void, size: libc::c_int) -> libc::c_int;
        fn proc_name(pid: libc::c_int, buffer: *mut libc::c_void, size: u32) -> libc::c_int;
    }

    #[allow(deprecated)]
    fn timebase() -> (u32, u32) {
        let mut tb = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: writes into the struct we pass.
        let rc = unsafe { libc::mach_timebase_info(&mut tb) };
        if rc != 0 || tb.denom == 0 {
            (1, 1)
        } else {
            (tb.numer, tb.denom)
        }
    }

    #[allow(deprecated)]
    pub fn host_now_ns() -> i64 {
        let (n, d) = timebase();
        // SAFETY: no preconditions.
        let t = unsafe { libc::mach_absolute_time() };
        i64::try_from(u128::from(t) * u128::from(n) / u128::from(d)).unwrap_or(i64::MAX)
    }

    pub fn info(pid: i32) -> Option<ProcInfo> {
        let mut ti = ProcTaskInfo::default();
        let sz = std::mem::size_of::<ProcTaskInfo>() as libc::c_int;
        // SAFETY: the buffer is a ProcTaskInfo of `sz` bytes, the layout of PROC_PIDTASKINFO.
        let r = unsafe {
            proc_pidinfo(
                pid,
                PROC_PIDTASKINFO,
                0,
                (&mut ti as *mut ProcTaskInfo).cast(),
                sz,
            )
        };
        if r != sz {
            return None;
        }
        let (n, d) = timebase();
        let ticks = (ti.pti_total_user + ti.pti_total_system) as f64;
        Some(ProcInfo {
            cpu_s: ticks * f64::from(n) / f64::from(d) / 1e9,
            rss_bytes: ti.pti_resident_size,
        })
    }

    /// `proc_name` returns up to 32 bytes, so long names still match by prefix.
    pub fn pids_named(prefix: &str) -> Vec<i32> {
        let mut buf = vec![0i32; 16384];
        // SAFETY: the buffer holds `buf.len()` pids.
        let n =
            unsafe { proc_listallpids(buf.as_mut_ptr().cast(), (buf.len() * 4) as libc::c_int) };
        let mut out = Vec::new();
        for &pid in buf.iter().take(n.max(0) as usize) {
            if pid <= 0 {
                continue;
            }
            let mut nb = [0u8; 256];
            // SAFETY: the buffer holds 256 bytes.
            let l = unsafe { proc_name(pid, nb.as_mut_ptr().cast(), 256) };
            if l > 0 && nb[..l as usize].starts_with(prefix.as_bytes()) {
                out.push(pid);
            }
        }
        out
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::ProcInfo;

    pub fn host_now_ns() -> i64 {
        0
    }

    pub fn info(_pid: i32) -> Option<ProcInfo> {
        None
    }

    pub fn pids_named(_prefix: &str) -> Vec<i32> {
        Vec::new()
    }
}

pub use imp::{host_now_ns, info, pids_named};

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn reads_own_process() {
        let me = std::process::id() as i32;
        let a = info(me).expect("own task info");
        assert!(a.rss_bytes > 1_000_000);
        let mut x = 0u64;
        for i in 0..20_000_000u64 {
            x = x.wrapping_add(i * i);
        }
        assert!(x > 0);
        let b = info(me).expect("own task info");
        assert!(b.cpu_s > a.cpu_s);
        let name = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let pids = pids_named(&name[..name.len().min(15)]);
        assert!(pids.contains(&me), "{name}: {pids:?}");
        let t0 = host_now_ns();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(host_now_ns() - t0 >= 15_000_000);
    }
}
