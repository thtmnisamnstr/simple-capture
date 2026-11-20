use std::fmt;

use crate::PermissionKind;

#[derive(Debug)]
pub enum Error {
    Unsupported(&'static str),
    PermissionDenied(PermissionKind),
    NotFound(String),
    Busy(String),
    Timeout(String),
    Os { code: i64, message: String },
    Io(std::io::Error),
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
            Error::PermissionDenied(k) => write!(f, "permission denied: {k:?}"),
            Error::NotFound(what) => write!(f, "not found: {what}"),
            Error::Busy(what) => write!(f, "busy: {what}"),
            Error::Timeout(what) => write!(f, "timeout: {what}"),
            Error::Os { code, message } => write!(f, "{message} (os error {code})"),
            Error::Io(e) => write!(f, "i/o error: {e}"),
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

impl From<sc_core::Error> for Error {
    fn from(e: sc_core::Error) -> Self {
        match e {
            sc_core::Error::Io(e) => Error::Io(e),
            sc_core::Error::Unsupported(what) => Error::Unsupported(what),
            other => Error::Other(other.to_string()),
        }
    }
}
