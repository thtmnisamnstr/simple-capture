#[cfg(target_os = "linux")]
mod gpu;
#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
fn main() {
    linux::run();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("spike 06 only runs on Linux");
}
