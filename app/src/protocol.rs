//! `sc://` scheme: compositor stills (`sc://still/<id>.png`, ARCHITECTURE §9). Stub until
//! wp-app-shell: every request is 404.

use std::borrow::Cow;

use tauri::http::{Request, Response, StatusCode};
use tauri::{Runtime, UriSchemeContext};

pub fn handle<R: Runtime>(
    _ctx: UriSchemeContext<'_, R>,
    _req: Request<Vec<u8>>,
) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Cow::Borrowed(&b"not found"[..]))
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(&b""[..])))
}
