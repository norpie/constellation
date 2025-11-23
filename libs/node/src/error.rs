use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Fabric error: {0}")]
    Fabric(#[from] constellation_fabric::error::Error),

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
