//! Helper (sidecar) processes and the files they consume (plan §4.6).

mod info;
mod locate;
mod process;

pub mod part;
pub mod wav;
pub mod y4m;

pub use info::{HelperInfo, helper_info};
pub use locate::{HelperKind, locate};
pub use process::{CancelReport, ExitReport, HelperProcess, Progress, SpawnSpec};
pub use sc_core::{Error, Result};
