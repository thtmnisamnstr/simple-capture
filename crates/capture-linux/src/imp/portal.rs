//! The xdg-desktop-portal ScreenCast picker. A pick keeps its portal session open; each stream
//! started from it gets its own PipeWire remote on that session.
//!
//! Picks persist (`persist_mode` 2): the portal returns a single-use restore token with every
//! pick, kept in the app's data folder, so the last screen or window can be opened again later
//! without the dialog. Each pick asks the portal for one PipeWire remote, right after the pick,
//! and keeps that connection: every stream of the pick (the size probe, each segment) runs on it,
//! so starting a recording never waits on the portal.

use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::{PersistMode, Session};
use sc_core::project::{Resolution, SourceDescriptor};
use sc_geometry::SizePx;
use sc_platform::{Error, PickedSource, PickerRequest, Result, SourcePicker, SourceSpec};

use super::pw::Connection;

/// The Linux app ID the portals know the app by; the installed desktop file must be named after
/// it. It differs from the bundle ID elsewhere because Flathub rejects IDs ending in `.desktop`.
pub const APP_ID: &str = "io.github.thtmnisamnstr.SimpleCapture";
const REMOTE_TIMEOUT: Duration = Duration::from_secs(5);
const NATIVE_WAIT: Duration = Duration::from_millis(1500);
const TOKEN_FILE: &str = "portal-screencast.txt";

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

/// Registers the app ID with the portal before its first call, as an unsandboxed app must for
/// the portals to keep its restore tokens and shortcuts under its own name. Inside Flatpak or
/// Snap (or with an older portal) the call fails and the sandbox's ID applies.
pub async fn ready() {
    static REGISTERED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    REGISTERED
        .get_or_init(|| async {
            let id = match APP_ID.parse() {
                Ok(id) => id,
                Err(e) => return tracing::warn!("portal app id: {e}"),
            };
            match ashpd::register_host_app(id).await {
                Ok(()) => tracing::info!(app_id = APP_ID, "portal: registered"),
                Err(e) => tracing::info!("portal: not registered ({e})"),
            }
        })
        .await;
}

static STATE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Where the restore token lives: the app's data folder.
pub fn set_state_dir(dir: PathBuf) {
    let _ = STATE_DIR.set(dir);
}

fn token_path() -> Option<PathBuf> {
    STATE_DIR.get().map(|d| d.join(TOKEN_FILE))
}

/// What the last pick was, for the recorder to name it without opening it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastPick {
    pub window: bool,
    pub size: SizePx,
}

fn read_token() -> Option<(String, LastPick)> {
    let text = std::fs::read_to_string(token_path()?).ok()?;
    let mut lines = text.lines();
    let token = lines.next()?.trim().to_string();
    let window = lines.next()? == "window";
    let (w, h) = lines.next()?.split_once('x')?;
    let size = SizePx {
        w: w.parse().ok()?,
        h: h.parse().ok()?,
    };
    (!token.is_empty()).then_some((token, LastPick { window, size }))
}

fn write_token(token: &str, last: LastPick) {
    let Some(path) = token_path() else { return };
    let text = format!(
        "{token}\n{}\n{}x{}\n",
        if last.window { "window" } else { "monitor" },
        last.size.w,
        last.size.h
    );
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = sc_core::fsio::atomic_write(&path, text.as_bytes()) {
        tracing::warn!("save the screen cast restore token: {e}");
    }
}

pub fn forget_last() {
    if let Some(p) = token_path() {
        let _ = std::fs::remove_file(p);
    }
}

pub fn last_pick() -> Option<LastPick> {
    read_token().map(|(_, l)| l)
}

fn portal_err(e: ashpd::Error) -> Error {
    Error::Other(format!("screen cast portal: {e}"))
}

pub struct Cast {
    proxy: Screencast,
    session: Option<Session<Screencast>>,
    conn: Mutex<Option<Arc<Connection>>>,
    native: OnceLock<SizePx>,
    pub node: u32,
    /// The logical size the portal reports.
    pub size: SizePx,
    pub window: bool,
}

impl Cast {
    async fn open_remote(&self) -> Result<OwnedFd> {
        let Some(session) = &self.session else {
            return Err(Error::Other("screen cast session closed".into()));
        };
        tokio::time::timeout(
            REMOTE_TIMEOUT,
            self.proxy
                .open_pipe_wire_remote(session, Default::default()),
        )
        .await
        .map_err(|_| Error::Timeout("the screen sharing service did not answer".into()))?
        .map_err(portal_err)
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Option<Arc<Connection>>> {
        self.conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The pick's PipeWire connection, opened once after the pick; a new remote is asked for
    /// only when that connection has gone.
    pub fn connection(self: &Arc<Self>) -> Result<Arc<Connection>> {
        if let Some(c) = self.held().as_ref().filter(|c| c.alive()) {
            return Ok(c.clone());
        }
        tracing::info!("screen cast: opening a PipeWire remote now");
        let fd = runtime().block_on(self.open_remote())?;
        let c = Connection::open(Some(fd))?;
        *self.held() = Some(c.clone());
        Ok(c)
    }

    /// The recording size: the source's real pixels for native resolution (found by a probe
    /// right after the pick), the portal's logical size for 1x or when the probe failed.
    pub fn size_for(&self, resolution: Resolution) -> SizePx {
        if resolution == Resolution::OneX {
            return self.size;
        }
        let deadline = std::time::Instant::now() + NATIVE_WAIT;
        loop {
            if let Some(s) = self.native.get() {
                return *s;
            }
            if std::time::Instant::now() >= deadline {
                tracing::warn!(
                    "screen cast: the native size is not known yet; using the logical size"
                );
                return self.size;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Opens the connection and finds the native size in the background, right after the pick.
    fn prepare(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        runtime().spawn(async move {
            let Some(c) = me.upgrade() else { return };
            let t = std::time::Instant::now();
            let fd = match c.open_remote().await {
                Ok(fd) => fd,
                Err(e) => {
                    tracing::warn!("screen cast: open a PipeWire remote: {e}");
                    let _ = c.native.set(c.size);
                    return;
                }
            };
            let node = c.node;
            let opened = tokio::task::spawn_blocking(move || {
                let conn = Connection::open(Some(fd))?;
                let native = conn.probe(node);
                Ok::<_, Error>((conn, native))
            })
            .await;
            match opened {
                Ok(Ok((conn, native))) => {
                    let native = native.unwrap_or(c.size);
                    tracing::info!(
                        ms = t.elapsed().as_millis(),
                        w = native.w,
                        h = native.h,
                        logical_w = c.size.w,
                        logical_h = c.size.h,
                        "screen cast: connection ready"
                    );
                    *c.held() = Some(conn);
                    let _ = c.native.set(native);
                }
                Ok(Err(e)) => {
                    tracing::warn!("screen cast: PipeWire connection: {e}");
                    let _ = c.native.set(c.size);
                }
                Err(e) => tracing::warn!("screen cast: {e}"),
            }
        });
    }
}

impl Drop for Cast {
    fn drop(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            runtime().spawn(async move {
                let _ = session.close().await;
            });
        } else {
            let _ = runtime().block_on(session.close());
        }
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

/// Shows the dialog, or with a restore token reopens that pick without it (the portal falls
/// back to the dialog when the token no longer applies).
async fn pick(restore: Option<String>) -> std::result::Result<Option<Cast>, ashpd::Error> {
    ready().await;
    let proxy = Screencast::new().await?;
    let session = proxy.create_session(Default::default()).await?;
    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::EMBEDDED)
                .set_sources(SourceType::MONITOR | SourceType::WINDOW)
                .set_multiple(false)
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(restore.as_deref()),
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
    let size = SizePx {
        w: (w.max(2) as u32) & !1,
        h: (h.max(2) as u32) & !1,
    };
    let window = stream
        .source_type()
        .is_some_and(|t| t.contains(SourceType::WINDOW));
    match response.restore_token() {
        Some(token) => write_token(token, LastPick { window, size }),
        None => forget_last(),
    }
    Ok(Some(Cast {
        node: stream.pipe_wire_node_id(),
        size,
        window,
        proxy,
        session: Some(session),
        conn: Mutex::new(None),
        native: OnceLock::new(),
    }))
}

fn picked(c: Cast) -> PickedSource {
    let descriptor = descriptor(&c);
    let c = Arc::new(c);
    c.prepare();
    PickedSource {
        descriptor,
        handle: c,
        region_pt: None,
    }
}

/// Reopens the last pick without the dialog; None when there is none to reopen.
pub fn restore_last() -> Result<Option<PickedSource>> {
    let Some((token, _)) = read_token() else {
        return Ok(None);
    };
    let started = std::time::Instant::now();
    let c = runtime().block_on(pick(Some(token))).map_err(portal_err)?;
    tracing::info!(
        ms = started.elapsed().as_millis(),
        "screen cast: restored the last pick"
    );
    Ok(c.map(picked))
}

pub struct PortalPicker;

impl SourcePicker for PortalPicker {
    fn present(
        &self,
        _req: PickerRequest,
        done: Box<dyn FnOnce(Result<Option<PickedSource>>) + Send>,
    ) {
        runtime().spawn(async move {
            let r = match pick(None).await {
                Ok(c) => Ok(c.map(picked)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_restore_token_round_trips() {
        let dir = std::env::temp_dir().join(format!("sc-portal-{}", sc_core::ids::new_id()));
        set_state_dir(dir.clone());
        let last = LastPick {
            window: true,
            size: SizePx { w: 1280, h: 720 },
        };
        write_token("tok-1", last);
        assert_eq!(read_token(), Some(("tok-1".to_string(), last)));
        assert_eq!(last_pick(), Some(last));
        forget_last();
        assert_eq!(read_token(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
