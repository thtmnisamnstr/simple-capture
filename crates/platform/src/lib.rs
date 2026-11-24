//! Platform interfaces (M1 plan §4.3). See the threading contract there: no method of a
//! stream, source, writer or reader pumps a run loop.

mod error;
mod media;
mod shared_camera;
mod sources;
mod traits;

pub mod synthetic;
pub mod unsupported;

pub use error::{Error, Result};
pub use media::{
    Append, AudioBuffer, AudioData, AudioTrackKind, AudioWriterSpec, CpuNv12Frame, DecodedFrame,
    FinishedTrack, FrameBuffer, NativeAudio, NativeFrame, Nv12View, TrackProbe, VideoCodec,
    VideoFrame, VideoWriterSpec,
};
pub use sc_core::ClockKind;
pub use sc_core::folder::LocalDateTime;
pub use sc_core::image::RgbaImage;
pub use sc_core::project::{Resolution, SourceDescriptor, SystemAudioMode};
pub use sc_core::time::HostTime;
pub use shared_camera::{SharedCamera, WithSharedCamera};
pub use sources::{
    AppInfo, AppRef, CameraLimit, Capabilities, CaptureDevice, DeviceKind, DisplayId, DisplayInfo,
    ExclusionKind, OverlayRole, PermissionKind, PermissionStatus, PickedSource, PickerRequest,
    ScreenStreamConfig, SourceEvent, SourceList, SourceSpec, StreamTag, WindowId, WindowInfo,
};
pub use traits::{
    AudioReader, AudioSource, CameraSource, Decoder, Desktop, DeviceStream, FrameSink,
    OverlayWindows, Permissions, Platform, Pointer, ScreenSource, ScreenStream, SourcePicker,
    SystemAudioSource, SystemAudioStream, TrackWriter, VideoReader, WriterFactory,
};
