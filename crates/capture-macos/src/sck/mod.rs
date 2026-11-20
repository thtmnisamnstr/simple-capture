//! ScreenCaptureKit sources, system audio, picker, permissions and AppKit glue.
//!
//! `NativeFrame::as_any` / `NativeAudio::as_any` of every buffer delivered here downcast to
//! `CFRetained<CMSampleBuffer>` ([`sample_buffer_of`]), so writers in `av` need no sck types.

mod audio;
mod content;
mod desktop;
mod frames;
mod glue;
mod overlays;
mod permissions;
mod picker;
mod stream;
mod system;

pub use audio::{MacSystemAudio, MacSystemAudioStream, pcm_f32_interleaved};
pub use content::{CurrentProcessReport, current_process_report};
pub use desktop::MacDesktop;
pub use frames::{ScAudio, ScFrame, audio_sample_buffer_of, sample_buffer_of};
pub use overlays::MacOverlays;
pub use permissions::MacPermissions;
pub use picker::{MacPicker, PickerConfigInfo};
pub use stream::{MacScreenSource, MacScreenStream, StreamStats};
pub use system::{
    capabilities, default_projects_dir, free_disk_bytes, local_now, macos_version, watch_system,
};
