//! Helper (sidecar) processes and the files they consume (plan §4.6).

mod info;
mod locate;
mod process;

pub mod part;
pub mod wav;
pub mod y4m;

pub use info::{HelperInfo, helper_info};
pub use locate::{HelperKind, bundled_path, locate, platform_dir_name};
pub use process::{
    CANCEL_GRACE, CancelReport, DEFAULT_QUEUE_FRAMES, ExitReport, HelperProcess, Interrupter,
    Progress, SpawnSpec, hide_console,
};
pub use sc_core::{Error, Result};
