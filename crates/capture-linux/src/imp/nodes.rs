//! Microphones as PipeWire sees them. cpal's PipeWire host lists every input-capable node,
//! including other apps' recording streams (`Stream/Input/Audio`) and, while we record, our own;
//! those are not microphones, and several streams share one node name.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use pipewire as pw;
use sc_capture_common::audio::mic_devices;
use sc_platform::{CaptureDevice, Result};

const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

/// An audio source as the session sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Source {
    description: String,
    muted: Option<bool>,
}

/// What cpal's device list leaves out: which nodes are streams, the sources' mute state (a muted
/// default microphone records silence in every app), and which source is the default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Snapshot {
    streams: HashSet<String>,
    sources: HashMap<String, Source>,
    default_source: Option<String>,
}

fn snapshot() -> Option<Snapshot> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("pipewire-nodes".into())
        .spawn(move || {
            let _ = tx.send(query());
        })
        .ok()?;
    rx.recv_timeout(QUERY_TIMEOUT).ok().flatten()
}

/// Runs the loop until the server has answered everything sent before this call.
fn roundtrip(mainloop: &pw::main_loop::MainLoopRc, core: &pw::core::CoreRc) -> Option<()> {
    let done = Rc::new(Cell::new(false));
    let pending = core.sync(0).ok()?;
    let _listener = core
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
    while !done.get() {
        mainloop.run();
    }
    Some(())
}

fn query() -> Option<Snapshot> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).ok()?;
    let context = pw::context::ContextRc::new(&mainloop, None).ok()?;
    let core = context.connect_rc(None).ok()?;
    let registry = core.get_registry_rc().ok()?;
    let snap = Rc::new(RefCell::new(Snapshot::default()));
    let nodes: Rc<RefCell<Vec<(pw::node::Node, pw::node::NodeListener)>>> = Rc::default();
    let metas: Rc<RefCell<Vec<(pw::metadata::Metadata, pw::metadata::MetadataListener)>>> =
        Rc::default();
    let _registry_listener = registry
        .add_listener_local()
        .global({
            let (snap, nodes, metas, registry) =
                (snap.clone(), nodes.clone(), metas.clone(), registry.clone());
            move |g| {
                let Some(props) = g.props else { return };
                match g.type_ {
                    pw::types::ObjectType::Node => {
                        let class = props.get(*pw::keys::MEDIA_CLASS).unwrap_or("");
                        let Some(name) = props.get(*pw::keys::NODE_NAME) else {
                            return;
                        };
                        if class.starts_with("Stream/") {
                            snap.borrow_mut().streams.insert(name.to_string());
                            return;
                        }
                        if !class.starts_with("Audio/Source") {
                            return;
                        }
                        snap.borrow_mut().sources.insert(
                            name.to_string(),
                            Source {
                                description: props
                                    .get(*pw::keys::NODE_DESCRIPTION)
                                    .unwrap_or(name)
                                    .to_string(),
                                muted: None,
                            },
                        );
                        let Ok(node) = registry.bind::<pw::node::Node, _>(g) else {
                            return;
                        };
                        let listener = node
                            .add_listener_local()
                            .param({
                                let (snap, name) = (snap.clone(), name.to_string());
                                move |_, id, _, _, param| {
                                    if id != pw::spa::param::ParamType::Props {
                                        return;
                                    }
                                    let muted = param
                                        .and_then(|p| p.as_object().ok())
                                        .and_then(|o| {
                                            o.find_prop(pw::spa::utils::Id(
                                                pw::spa::sys::SPA_PROP_mute,
                                            ))
                                        })
                                        .and_then(|prop| prop.value().get_bool().ok());
                                    if let Some(m) = muted
                                        && let Some(s) = snap.borrow_mut().sources.get_mut(&name)
                                    {
                                        s.muted = Some(m);
                                    }
                                }
                            })
                            .register();
                        node.enum_params(0, Some(pw::spa::param::ParamType::Props), 0, u32::MAX);
                        nodes.borrow_mut().push((node, listener));
                    }
                    pw::types::ObjectType::Metadata => {
                        if props.get("metadata.name") != Some("default") {
                            return;
                        }
                        let Ok(meta) = registry.bind::<pw::metadata::Metadata, _>(g) else {
                            return;
                        };
                        let listener = meta
                            .add_listener_local()
                            .property({
                                let snap = snap.clone();
                                move |_, key, _, value| {
                                    if key == Some("default.audio.source")
                                        && let Some(name) = value.and_then(json_name)
                                    {
                                        snap.borrow_mut().default_source = Some(name);
                                    }
                                    0
                                }
                            })
                            .register();
                        metas.borrow_mut().push((meta, listener));
                    }
                    _ => {}
                }
            }
        })
        .register();
    roundtrip(&mainloop, &core)?;
    roundtrip(&mainloop, &core)?;
    let out = snap.borrow().clone();
    Some(out)
}

/// The `name` in a metadata value such as `{ "name": "alsa_input.pci-0000" }`.
fn json_name(value: &str) -> Option<String> {
    let rest = &value[value.find("\"name\"")? + 6..];
    let rest = &rest[rest.find('"')? + 1..];
    Some(rest[..rest.find('"')?].to_string())
}

pub fn mics() -> Result<Vec<CaptureDevice>> {
    let devices = mic_devices()?;
    Ok(only_mics(devices, &snapshot().unwrap_or_default()))
}

const MUTED: &str = " (muted in system settings)";

/// Leaves out streams and repeated ids, names the default after the source it stands for, and
/// says when a source is muted.
fn only_mics(devices: Vec<CaptureDevice>, snap: &Snapshot) -> Vec<CaptureDevice> {
    let mut seen = HashSet::new();
    devices
        .into_iter()
        .filter(|d| {
            let node = d.id.strip_prefix("pipewire:").unwrap_or(&d.id);
            !snap.streams.contains(node) && seen.insert(d.id.clone())
        })
        .map(|mut d| {
            let node = d.id.strip_prefix("pipewire:").unwrap_or(&d.id);
            let source = if node == "input_default" {
                let real = snap
                    .default_source
                    .as_ref()
                    .and_then(|n| snap.sources.get(n));
                d.name = match real {
                    Some(s) => format!("System default ({})", s.description),
                    None => "System default".to_string(),
                };
                real
            } else {
                snap.sources.get(node)
            };
            if source.and_then(|s| s.muted) == Some(true) {
                d.name.push_str(MUTED);
            }
            d
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, name: &str) -> CaptureDevice {
        CaptureDevice {
            id: id.into(),
            name: name.into(),
            is_default: false,
            model_id: None,
            kind_hint: None,
            unavailable: None,
        }
    }

    #[test]
    fn streams_go_the_default_is_named_and_muted_sources_say_so() {
        let snap = Snapshot {
            streams: [
                "simple-capture-app".to_string(),
                "audio_effect.j313-mic".to_string(),
            ]
            .into_iter()
            .collect(),
            sources: [
                (
                    "effect_output.j313-mic".to_string(),
                    Source {
                        description: "MacBook Air J313 Microphone".into(),
                        muted: Some(true),
                    },
                ),
                (
                    "usb-mic".to_string(),
                    Source {
                        description: "USB Mic".into(),
                        muted: Some(false),
                    },
                ),
            ]
            .into_iter()
            .collect(),
            default_source: Some("effect_output.j313-mic".into()),
        };
        let out = only_mics(
            vec![
                dev("pipewire:input_default", "default_input"),
                dev("pipewire:simple-capture-app", "simple-capture-app"),
                dev(
                    "pipewire:audio_effect.j313-mic",
                    "MacBook Air J313 Microphone",
                ),
                dev(
                    "pipewire:effect_output.j313-mic",
                    "MacBook Air J313 Microphone",
                ),
                dev("pipewire:usb-mic", "USB Mic"),
                dev("pipewire:usb-mic", "USB Mic"),
            ],
            &snap,
        );
        let names: Vec<(&str, &str)> = out
            .iter()
            .map(|d| (d.id.as_str(), d.name.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                (
                    "pipewire:input_default",
                    "System default (MacBook Air J313 Microphone) (muted in system settings)"
                ),
                (
                    "pipewire:effect_output.j313-mic",
                    "MacBook Air J313 Microphone (muted in system settings)"
                ),
                ("pipewire:usb-mic", "USB Mic"),
            ]
        );
    }

    #[test]
    fn the_default_source_name_comes_out_of_its_metadata_value() {
        assert_eq!(
            json_name(r#"{ "name": "effect_output.j313-mic" }"#).as_deref(),
            Some("effect_output.j313-mic")
        );
        assert_eq!(json_name("{}"), None);
    }
}
