//! Error type for the crate.

use alloc::string::String;
use core::fmt;

/// Errors produced while loading a transducer or applying it to input.
#[derive(Debug)]
pub enum Error {
    /// The bytes could not be parsed as an optimized-lookup transducer.
    Format(String),
    /// A feature is recognised but not yet implemented in this port.
    Unsupported(&'static str),
    /// Underlying I/O error while reading a transducer file. Only produced by
    /// [`crate::Transducer::from_path`], which needs the `std` feature.
    #[cfg(feature = "std")]
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Format(m) => write!(f, "format error: {m}"),
            Error::Unsupported(m) => write!(f, "unsupported: {m}"),
            #[cfg(feature = "std")]
            Error::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = core::result::Result<T, Error>;
