//! The xdg-desktop-portal ScreenCast picker. A pick keeps its portal session open; each stream
//! started from it opens its own PipeWire remote on that session.

use std::os::fd::OwnedFd;
use std::sync::{Arc, OnceLock};

use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::{PersistMode, Session};
use sc_core::project::SourceDescriptor;
use sc_geometry::SizePx;
use sc_platform::{Error, PickedSource, PickerRequest, Result, SourcePicker, SourceSpec};

pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("portal")
            .enable_all()
            .build()
            .expect("portal runtime")
    })
}

fn portal_err(e: ashpd::Error) -> Error {
    Error::Other(format!("screen cast portal: {e}"))
}

pub struct Cast {
    proxy: Screencast,
    session: Session<Screencast>,
    pub node: u32,
    pub size: SizePx,
    pub window: bool,
}

impl Cast {
    pub fn remote(&self) -> Result<OwnedFd> {
        runtime()
            .block_on(
                self.proxy
                    .open_pipe_wire_remote(&self.session, Default::default()),
            )
            .map_err(portal_err)
    }
}

impl Drop for Cast {
    fn drop(&mut self) {
        let _ = runtime().block_on(self.session.close());
    }
}

pub fn cast_of(spec: &SourceSpec) -> Result<Arc<Cast>> {
    match spec {
        SourceSpec::Picked(p) => p
            .handle
            .clone()
            .downcast::<Cast>()
            .map_err(|_| Error::Other("picked source: foreign handle".into())),
        _ => Err(Error::Unsupported(
            "screens and windows are chosen with the system picker on Linux",
        )),
    }
}

pub fn descriptor(c: &Cast) -> SourceDescriptor {
    if c.window {
        SourceDescriptor::Window {
            window_id: c.node,
            app_bundle_id: None,
            app_name: None,
            window_title: None,
            scale: 1.0,
        }
    } else {
        SourceDescriptor::Display {
            display_id: format!("portal:{}", c.node),
            display_name: Some("Screen".into()),
            scale: 1.0,
            frame_pt: None,
        }
    }
}

async fn pick() -> std::result::Result<Option<Cast>, ashpd::Error> {
    let proxy = Screencast::new().await?;
    let session = proxy.create_session(Default::default()).await?;
    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::EMBEDDED)
                .set_sources(SourceType::MONITOR | SourceType::WINDOW)
                .set_multiple(false)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await?;
    let response = match proxy
        .start(&session, None, Default::default())
        .await?
        .response()
    {
        Ok(r) => r,
        Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => {
            let _ = session.close().await;
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    let Some(stream) = response.streams().first() else {
        let _ = session.close().await;
        return Ok(None);
    };
    let (w, h) = stream.size().unwrap_or((1920, 1080));
    Ok(Some(Cast {
        node: stream.pipe_wire_node_id(),
        size: SizePx {
            w: (w.max(2) as u32) & !1,
            h: (h.max(2) as u32) & !1,
        },
        window: stream
            .source_type()
            .is_some_and(|t| t.contains(SourceType::WINDOW)),
        proxy,
        session,
    }))
}

pub struct PortalPicker;

impl SourcePicker for PortalPicker {
    fn present(
        &self,
        _req: PickerRequest,
        done: Box<dyn FnOnce(Result<Option<PickedSource>>) + Send>,
    ) {
        runtime().spawn(async move {
            let r = match pick().await {
                Ok(Some(c)) => {
                    let descriptor = descriptor(&c);
                    Ok(Some(PickedSource {
                        descriptor,
                        handle: Arc::new(c),
                        region_pt: None,
                    }))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(portal_err(e)),
            };
            done(r);
        });
    }

    fn set_observer(&self, _on_pick: Option<Box<dyn Fn(PickedSource) + Send + Sync>>) {}

    fn set_active(&self, _active: bool) -> Result<()> {
        Ok(())
    }
}
