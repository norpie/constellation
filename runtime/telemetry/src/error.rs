//! Error types for the Telemetry Service

use constellation_node::error::Error as NodeError;
use serde::Serialize;
use thiserror::Error;

/// Telemetry service error type
#[derive(Error, Debug, Serialize)]
pub enum Error {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Query error: {0}")]
    Query(String),
}

impl From<constellation_datapad::Error> for Error {
    fn from(e: constellation_datapad::Error) -> Self {
        Error::Storage(e.to_string())
    }
}

impl From<Error> for NodeError {
    fn from(e: Error) -> Self {
        NodeError::Custom(e.to_string())
    }
}
