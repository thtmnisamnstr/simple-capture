use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::path::Path;

pub const SIGKILL: c_int = 9;
pub const SIGTERM: c_int = 15;
pub const SIGSTOP: c_int = 17;
const WNOHANG: c_int = 1;
const ESRCH: i32 = 3;

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct TimeVal {
    pub sec: i64,
    pub usec: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct RUsage {
    pub utime: TimeVal,
    pub stime: TimeVal,
    pub maxrss: i64,
    pub rest: [i64; 13],
}

impl RUsage {
    pub fn cpu_s(&self) -> f64 {
        let t = |v: TimeVal| v.sec as f64 + v.usec as f64 / 1e6;
        t(self.utime) + t(self.stime)
    }
}

unsafe extern "C" {
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn wait4(pid: c_int, status: *mut c_int, options: c_int, rusage: *mut RUsage) -> c_int;
    fn mkfifo(path: *const c_char, mode: u16) -> c_int;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Signal(i32),
}

impl Exit {
    pub fn success(&self) -> bool {
        matches!(self, Exit::Code(0))
    }
    pub fn describe(&self) -> String {
        match self {
            Exit::Code(c) => format!("exit {c}"),
            Exit::Signal(s) => format!("signal {s}"),
        }
    }
}

pub fn send_signal(pid: u32, sig: c_int) -> bool {
    unsafe { kill(pid as c_int, sig) == 0 }
}

pub fn pid_alive(pid: u32) -> bool {
    let r = unsafe { kill(pid as c_int, 0) };
    !(r != 0 && std::io::Error::last_os_error().raw_os_error() == Some(ESRCH))
}

pub fn try_reap(pid: u32) -> Option<(Exit, RUsage)> {
    let mut status: c_int = 0;
    let mut ru = RUsage::default();
    let r = unsafe { wait4(pid as c_int, &mut status, WNOHANG, &mut ru) };
    if r == pid as c_int {
        let sig = status & 0x7f;
        let exit = if sig == 0 { Exit::Code((status >> 8) & 0xff) } else { Exit::Signal(sig) };
        Some((exit, ru))
    } else {
        None
    }
}

pub fn make_fifo(path: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(path);
    let c = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    if unsafe { mkfifo(c.as_ptr(), 0o600) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub fn clear_nonblocking(fd: c_int) {
    const F_GETFL: c_int = 3;
    const F_SETFL: c_int = 4;
    const O_NONBLOCK: c_int = 4;
    unsafe {
        let fl = fcntl(fd, F_GETFL);
        fcntl(fd, F_SETFL, fl & !O_NONBLOCK);
    }
}

