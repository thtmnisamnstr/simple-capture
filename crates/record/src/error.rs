use std::fmt;

#[derive(Debug)]
pub enum Error {
    Platform(sc_platform::Error),
    Core(sc_core::Error),
    State(String),
    Unsupported(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Platform(e) => write!(f, "{e}"),
            Error::Core(e) => write!(f, "{e}"),
            Error::State(e) => write!(f, "invalid state: {e}"),
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Platform(e) => Some(e),
            Error::Core(e) => Some(e),
            _ => None,
        }
    }
}

impl From<sc_platform::Error> for Error {
    fn from(e: sc_platform::Error) -> Self {
        Error::Platform(e)
    }
}

impl From<sc_core::Error> for Error {
    fn from(e: sc_core::Error) -> Self {
        Error::Core(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Core(sc_core::Error::Io(e))
    }
}
