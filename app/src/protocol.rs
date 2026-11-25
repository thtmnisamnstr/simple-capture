//! `sc://` scheme: compositor stills (`sc://still/<id>.png`, ARCHITECTURE §9) and live frames
//! (`sc://live/<name>?after=<id>`, see `live.rs`).

use std::borrow::Cow;
use std::sync::Arc;

use tauri::http::{Request, Response, StatusCode, header};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

use crate::core::Core;
use crate::still;

fn respond(
    status: StatusCode,
    ctype: &str,
    body: Cow<'static, [u8]>,
) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, ctype)
        .header(header::CACHE_CONTROL, "no-store")
        .body(body)
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(&b""[..])))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), b.get(i + 2).and_then(|c| hex(*c)))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `sc://thumb/<step_ms>/<index>.png` (`cthumb` for the camera) and `sc://peaks/v1` of the project open in the editor.
fn editor_file(
    core: &Core,
    host: Option<&str>,
    path: &str,
) -> Option<Response<Cow<'static, [u8]>>> {
    let (host, path) = match host {
        Some("localhost") | None => {
            let p = path.trim_start_matches('/');
            let (h, rest) = p.split_once('/').unwrap_or((p, ""));
            (h.to_string(), format!("/{rest}"))
        }
        Some(h) => (h.to_string(), path.to_string()),
    };
    if host == "font" {
        let name = percent_decode(path.trim_start_matches('/'));
        let name = name.as_str();
        let ok = sc_text::fonts::BUNDLED
            .iter()
            .flat_map(|f| f.faces)
            .any(|f| f.file == name);
        let bytes = ok
            .then(|| std::fs::read(sc_text::fonts::default_fonts_dir().join(name)).ok())
            .flatten()?;
        return Some(
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "font/ttf")
                .header(header::CACHE_CONTROL, "max-age=86400")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(Cow::Owned(bytes))
                .unwrap_or_else(|_| Response::new(Cow::Borrowed(&b""[..]))),
        );
    }
    if host == "poster" {
        let name = percent_decode(path.trim_start_matches('/'));
        let name = name.strip_suffix(".png")?;
        let dir = crate::library::project_in(&core.projects_dir(), name)?;
        return Some(
            match crate::library::poster(&dir, core.platform.decoder()) {
                Ok(file) => match std::fs::read(&file) {
                    Ok(bytes) => Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "image/png")
                        .header(header::CACHE_CONTROL, "max-age=600")
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .body(Cow::Owned(bytes))
                        .unwrap_or_else(|_| Response::new(Cow::Borrowed(&b""[..]))),
                    Err(_) => respond(
                        StatusCode::NOT_FOUND,
                        "text/plain",
                        Cow::Borrowed(&b"not found"[..]),
                    ),
                },
                Err(e) => {
                    tracing::warn!(name, "poster: {e}");
                    respond(
                        StatusCode::NOT_FOUND,
                        "text/plain",
                        Cow::Borrowed(&b"no poster"[..]),
                    )
                }
            },
        );
    }
    let dir = core.editor_dir()?;
    let (file, ctype) = match host.as_str() {
        "thumb" | "cthumb" => {
            let lane = if host == "cthumb" {
                crate::editor_media::Lane::Camera
            } else {
                crate::editor_media::Lane::Screen
            };
            let mut parts = path.trim_start_matches('/').split('/');
            let step: i64 = parts.next()?.parse().ok()?;
            let index: i64 = parts.next()?.strip_suffix(".png")?.parse().ok()?;
            (
                crate::editor_media::lane_thumb_path(&dir, lane, step, index),
                "image/png",
            )
        }
        "peaks" => (
            crate::editor_media::peaks_path(&dir, path.trim_start_matches('/')),
            "application/octet-stream",
        ),
        _ => return None,
    };
    Some(match std::fs::read(&file) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, ctype)
            .header(header::CACHE_CONTROL, "max-age=3600")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(Cow::Owned(bytes))
            .unwrap_or_else(|_| Response::new(Cow::Borrowed(&b""[..]))),
        Err(_) => respond(
            StatusCode::NOT_FOUND,
            "text/plain",
            Cow::Borrowed(&b"not found"[..]),
        ),
    })
}

pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    req: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let core = ctx
        .app_handle()
        .try_state::<Arc<Core>>()
        .map(|s| s.inner().clone());
    let uri = req.uri().clone();
    std::thread::spawn(move || {
        if let Some((name, after)) = crate::live::parse(uri.host(), uri.path(), uri.query()) {
            let frame = core.as_ref().and_then(|c| {
                c.live
                    .next(&name, after, std::time::Duration::from_millis(500))
            });
            let resp = match frame {
                Some(f) => Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/octet-stream")
                    .header(header::CACHE_CONTROL, "no-store")
                    .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                    .body(Cow::Owned(f.body.to_vec()))
                    .unwrap_or_else(|_| Response::new(Cow::Borrowed(&b""[..]))),
                None => Response::builder()
                    .status(StatusCode::NO_CONTENT)
                    .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                    .body(Cow::Borrowed(&b""[..]))
                    .unwrap_or_else(|_| Response::new(Cow::Borrowed(&b""[..]))),
            };
            responder.respond(resp);
            return;
        }
        if let Some(resp) = core
            .as_ref()
            .and_then(|c| editor_file(c, uri.host(), uri.path()))
        {
            responder.respond(resp);
            return;
        }
        let png = still::id_from_path(uri.host(), uri.path())
            .and_then(|id| core.as_ref().and_then(|c| c.still_png(&id)));
        let resp = match png {
            Some(bytes) => respond(StatusCode::OK, "image/png", Cow::Owned(bytes.to_vec())),
            None => respond(
                StatusCode::NOT_FOUND,
                "text/plain",
                Cow::Borrowed(&b"not found"[..]),
            ),
        };
        responder.respond(resp);
    });
}
