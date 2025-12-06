//! Datapad error types.

use std::fmt;

/// Datapad operation error.
#[derive(Debug)]
pub enum Error {
    /// Storage engine error.
    Storage(sled::Error),

    /// Serialization error.
    Serialization(String),

    /// Entry not found.
    NotFound,

    /// Invalid key format.
    InvalidKey,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Storage(e) => write!(f, "storage error: {}", e),
            Error::Serialization(msg) => write!(f, "serialization error: {}", msg),
            Error::NotFound => write!(f, "entry not found"),
            Error::InvalidKey => write!(f, "invalid key format"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Storage(e) => Some(e),
            _ => None,
        }
    }
}

impl From<sled::Error> for Error {
    fn from(e: sled::Error) -> Self {
        Error::Storage(e)
    }
}

/// Result type for Datapad operations.
pub type Result<T> = std::result::Result<T, Error>;
