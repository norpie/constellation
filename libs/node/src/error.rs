use serde::Serialize;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug, Serialize)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(String),

    #[error("Fabric error: {0}")]
    Fabric(String),

    #[error("Raft error: {0}")]
    Raft(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Route not found: {0}")]
    RouteNotFound(String),

    #[error("No compatible transport for route: {0}")]
    NoCompatibleTransport(String),

    #[error("Missing dependency: {0}")]
    MissingDependency(String),

    #[error("{0}")]
    Custom(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

impl From<constellation_fabric::error::Error> for Error {
    fn from(e: constellation_fabric::error::Error) -> Self {
        Error::Fabric(e.to_string())
    }
}

impl From<constellation_raft::Error> for Error {
    fn from(e: constellation_raft::Error) -> Self {
        Error::Raft(e.to_string())
    }
}

impl crate::rpc::ErrorResponder for Error {
    fn error_category(&self) -> crate::rpc::ErrorCategory {
        match self {
            // Client errors - bad request, don't retry
            Error::Serialization(_) | Error::RouteNotFound(_) => {
                crate::rpc::ErrorCategory::ClientError
            }
            // Server errors - internal issues, may retry
            Error::Io(_)
            | Error::Fabric(_)
            | Error::Raft(_)
            | Error::NoCompatibleTransport(_)
            | Error::MissingDependency(_)
            | Error::Custom(_) => crate::rpc::ErrorCategory::ServerError,
        }
    }
}
