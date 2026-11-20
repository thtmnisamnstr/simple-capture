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
