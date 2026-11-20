//! `sc://` scheme: compositor stills (`sc://still/<id>.png`, ARCHITECTURE §9).

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
