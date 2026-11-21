//! Microphones and system audio through cpal. System audio opens an input stream on the
//! default output device: WASAPI loopback on Windows, a PipeWire sink capture on Linux.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Data, SampleFormat};
use sc_core::project::SystemAudioMode;
use sc_platform::{
    AppRef, AudioBuffer, AudioData, AudioSource, AudioTrackKind, CaptureDevice, DeviceKind,
    DeviceStream, Error, FrameSink, HostTime, Result, SourceEvent, SystemAudioSource,
    SystemAudioStream, WindowId,
};

pub type Clock = fn() -> HostTime;

pub const DEFAULT_DEVICE: &str = "default";

fn host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    if let Ok(h) = cpal::host_from_id(cpal::HostId::PipeWire) {
        return h;
    }
    cpal::default_host()
}

fn device_id(d: &cpal::Device) -> String {
    d.id()
        .map(|i| i.to_string())
        .unwrap_or_else(|_| d.to_string())
}

fn device_name(d: &cpal::Device) -> String {
    d.description()
        .map(|desc| desc.name().to_string())
        .unwrap_or_else(|_| d.to_string())
}

fn is_mic(d: &cpal::Device) -> bool {
    d.description()
        .map(|desc| desc.direction() == cpal::DeviceDirection::Input)
        .unwrap_or_else(|_| d.supports_input())
}

pub fn mic_devices() -> Result<Vec<CaptureDevice>> {
    let host = host();
    let default = host.default_input_device().map(|d| device_id(&d));
    let devices = host
        .input_devices()
        .map_err(|e| Error::Other(format!("list microphones: {e}")))?;
    Ok(devices
        .filter(is_mic)
        .map(|d| {
            let id = device_id(&d);
            CaptureDevice {
                is_default: default.as_deref() == Some(id.as_str()),
                name: device_name(&d),
                id,
                model_id: None,
                kind_hint: None,
            }
        })
        .collect())
}

fn find_input(id: &str) -> Result<cpal::Device> {
    let host = host();
    if id == DEFAULT_DEVICE || id.is_empty() {
        return host
            .default_input_device()
            .ok_or_else(|| Error::NotFound("no default microphone".into()));
    }
    host.input_devices()
        .map_err(|e| Error::Other(format!("list microphones: {e}")))?
        .find(|d| device_id(d) == id)
        .ok_or_else(|| Error::NotFound(format!("microphone {id}")))
}

fn to_f32(data: &Data) -> Option<Vec<f32>> {
    Some(match data.sample_format() {
        SampleFormat::F32 => data.as_slice::<f32>()?.to_vec(),
        SampleFormat::F64 => data.as_slice::<f64>()?.iter().map(|v| *v as f32).collect(),
        SampleFormat::I16 => data
            .as_slice::<i16>()?
            .iter()
            .map(|v| f32::from(*v) / 32_768.0)
            .collect(),
        SampleFormat::I32 => data
            .as_slice::<i32>()?
            .iter()
            .map(|v| *v as f32 / 2_147_483_648.0)
            .collect(),
        SampleFormat::U16 => data
            .as_slice::<u16>()?
            .iter()
            .map(|v| (f32::from(*v) - 32_768.0) / 32_768.0)
            .collect(),
        SampleFormat::U8 => data
            .as_slice::<u8>()?
            .iter()
            .map(|v| (f32::from(*v) - 128.0) / 128.0)
            .collect(),
        SampleFormat::I8 => data
            .as_slice::<i8>()?
            .iter()
            .map(|v| f32::from(*v) / 128.0)
            .collect(),
        _ => return None,
    })
}

struct Running {
    stop: mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
    format: String,
}

impl Running {
    fn stop(mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

type Deliver = Arc<dyn Fn(AudioBuffer) + Send + Sync>;
type OnError = Arc<dyn Fn(String) + Send + Sync>;

/// Builds and plays the stream on its own thread (a cpal stream is not `Send` everywhere) and
/// keeps it until stopped.
fn run(
    label: &'static str,
    open: impl FnOnce() -> Result<cpal::Device> + Send + 'static,
    clock: Clock,
    deliver: Deliver,
    on_error: OnError,
) -> Result<Running> {
    let (ready_tx, ready_rx) = mpsc::channel::<Result<String>>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name(format!("audio-{label}"))
        .spawn(move || {
            let built = (|| -> Result<(cpal::Stream, String)> {
                let device = open()?;
                let cfg = device
                    .default_input_config()
                    .or_else(|_| device.default_output_config())
                    .map_err(|e| Error::Other(format!("{label}: no input format: {e}")))?;
                let format = cfg.sample_format();
                let config: cpal::StreamConfig = cfg.into();
                let rate = config.sample_rate;
                let channels = config.channels.max(1);
                let deliver = deliver.clone();
                let on_error = on_error.clone();
                let stream = device
                    .build_input_stream_raw(
                        config,
                        format,
                        move |data: &Data, _| {
                            let Some(samples) = to_f32(data) else {
                                return;
                            };
                            let frames = (samples.len() / usize::from(channels)) as u32;
                            let dur = i64::from(frames) * 1_000_000_000 / i64::from(rate.max(1));
                            deliver(AudioBuffer {
                                pts: HostTime(clock().0 - dur),
                                sample_rate: rate,
                                channels,
                                frames,
                                data: AudioData::F32Interleaved(samples.into()),
                            });
                        },
                        move |e| on_error(e.to_string()),
                        None,
                    )
                    .map_err(|e| Error::Other(format!("{label}: open stream: {e}")))?;
                stream
                    .play()
                    .map_err(|e| Error::Other(format!("{label}: start: {e}")))?;
                Ok((
                    stream,
                    format!(
                        "{} {rate} Hz {channels} ch {format:?}",
                        device_name(&device)
                    ),
                ))
            })();
            match built {
                Ok((stream, format)) => {
                    let _ = ready_tx.send(Ok(format));
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })?;
    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(format)) => Ok(Running {
            stop: stop_tx,
            thread: Some(thread),
            format,
        }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(Error::Timeout(format!("{label}: the stream did not start"))),
    }
}

pub struct AudioStream(Option<Running>);

impl DeviceStream for AudioStream {
    fn format(&self) -> String {
        self.0
            .as_ref()
            .map(|r| r.format.clone())
            .unwrap_or_default()
    }

    fn stop(mut self: Box<Self>) -> Result<()> {
        if let Some(r) = self.0.take() {
            r.stop();
        }
        Ok(())
    }
}

impl SystemAudioStream for AudioStream {
    fn follow_app(&self, app: Option<&AppRef>) -> Result<()> {
        match app {
            None => Ok(()),
            Some(_) => Err(Error::Unsupported("system audio from one app")),
        }
    }

    fn stop(mut self: Box<Self>) -> Result<()> {
        if let Some(r) = self.0.take() {
            r.stop();
        }
        Ok(())
    }
}

pub struct CpalMic {
    pub clock: Clock,
}

impl AudioSource for CpalMic {
    fn devices(&self) -> Result<Vec<CaptureDevice>> {
        mic_devices()
    }

    fn start(&self, device_id: &str, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
        let id = device_id.to_string();
        let s = sink.clone();
        let lost = id.clone();
        let running = run(
            "mic",
            move || find_input(&id),
            self.clock,
            Arc::new(move |b| s.on_audio(AudioTrackKind::Mic, b)),
            Arc::new(move |e| {
                tracing::warn!("microphone: {e}");
                sink.on_event(SourceEvent::DeviceLost {
                    kind: DeviceKind::Mic,
                    device_id: lost.clone(),
                });
            }),
        )?;
        Ok(Box::new(AudioStream(Some(running))))
    }

    fn show_mic_modes(&self) -> Result<()> {
        Err(Error::Unsupported("microphone modes"))
    }
}

pub struct CpalSystemAudio {
    pub clock: Clock,
}

impl SystemAudioSource for CpalSystemAudio {
    fn start(
        &self,
        mode: SystemAudioMode,
        _exclude: &[WindowId],
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn SystemAudioStream>> {
        if mode == SystemAudioMode::Off {
            return Err(Error::Other("system audio is off".into()));
        }
        let s = sink.clone();
        let running = run(
            "system",
            || {
                host()
                    .default_output_device()
                    .ok_or_else(|| Error::NotFound("no output device to capture".into()))
            },
            self.clock,
            Arc::new(move |b| s.on_audio(AudioTrackKind::SystemAudio, b)),
            Arc::new(|e| tracing::warn!("system audio: {e}")),
        )?;
        Ok(Box::new(AudioStream(Some(running))))
    }
}
