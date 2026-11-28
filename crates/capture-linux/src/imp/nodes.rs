//! Microphones as PipeWire sees them. cpal's PipeWire host lists every input-capable node,
//! including other apps' recording streams (`Stream/Input/Audio`) and, while we record, our own;
//! those are not microphones, and several streams share one node name.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use pipewire as pw;
use sc_capture_common::audio::mic_devices;
use sc_platform::{CaptureDevice, Result};

const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

fn stream_node_names() -> Option<HashSet<String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("pipewire-nodes".into())
        .spawn(move || {
            let _ = tx.send(query_stream_nodes());
        })
        .ok()?;
    rx.recv_timeout(QUERY_TIMEOUT).ok().flatten()
}

fn query_stream_nodes() -> Option<HashSet<String>> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).ok()?;
    let context = pw::context::ContextRc::new(&mainloop, None).ok()?;
    let core = context.connect_rc(None).ok()?;
    let registry = core.get_registry().ok()?;
    let names = Rc::new(RefCell::new(HashSet::new()));
    let done = Rc::new(Cell::new(false));
    let pending = core.sync(0).ok()?;
    let _core_listener = core
        .add_listener_local()
        .done({
            let (done, mainloop) = (done.clone(), mainloop.clone());
            move |id, seq| {
                if id == pw::core::PW_ID_CORE && seq == pending {
                    done.set(true);
                    mainloop.quit();
                }
            }
        })
        .register();
    let _registry_listener = registry
        .add_listener_local()
        .global({
            let names = names.clone();
            move |g| {
                if g.type_ != pw::types::ObjectType::Node {
                    return;
                }
                let Some(props) = g.props else { return };
                if props
                    .get(*pw::keys::MEDIA_CLASS)
                    .is_some_and(|c| c.starts_with("Stream/"))
                    && let Some(name) = props.get(*pw::keys::NODE_NAME)
                {
                    names.borrow_mut().insert(name.to_string());
                }
            }
        })
        .register();
    while !done.get() {
        mainloop.run();
    }
    Some(names.take())
}

pub fn mics() -> Result<Vec<CaptureDevice>> {
    let devices = mic_devices()?;
    let streams = stream_node_names().unwrap_or_default();
    Ok(only_mics(devices, &streams))
}

fn only_mics(devices: Vec<CaptureDevice>, streams: &HashSet<String>) -> Vec<CaptureDevice> {
    let mut seen = HashSet::new();
    devices
        .into_iter()
        .filter(|d| {
            let node = d.id.strip_prefix("pipewire:").unwrap_or(&d.id);
            !streams.contains(node) && seen.insert(d.id.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str) -> CaptureDevice {
        CaptureDevice {
            id: id.into(),
            name: id.into(),
            is_default: false,
            model_id: None,
            kind_hint: None,
            unavailable: None,
        }
    }

    #[test]
    fn recording_streams_and_repeated_ids_are_not_microphones() {
        let streams: HashSet<String> = ["simple-capture-app".to_string(), "Firefox".to_string()]
            .into_iter()
            .collect();
        let out = only_mics(
            vec![
                dev("pipewire:input_default"),
                dev("pipewire:simple-capture-app"),
                dev("pipewire:audio_effect.j313-mic"),
                dev("pipewire:simple-capture-app"),
                dev("pipewire:Firefox"),
                dev("pipewire:audio_effect.j313-mic"),
            ],
            &streams,
        );
        let ids: Vec<&str> = out.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(
            ids,
            ["pipewire:input_default", "pipewire:audio_effect.j313-mic"]
        );
    }
}
