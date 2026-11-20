use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(String),
    Invalid(String),
    Locked(PathBuf),
    Unsupported(&'static str),
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "i/o error: {e}"),
            Error::Json(e) => write!(f, "json error: {e}"),
            Error::Invalid(e) => write!(f, "invalid: {e}"),
            Error::Locked(p) => write!(f, "locked: {}", p.display()),
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
            Error::Other(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e.to_string())
    }
}

impl Error {
    /// An `Io` error whose message names the operation and the path.
    pub(crate) fn io_at(e: std::io::Error, what: &str, path: &std::path::Path) -> Error {
        Error::Io(std::io::Error::new(
            e.kind(),
            format!("{what} {}: {e}", path.display()),
        ))
    }
}
