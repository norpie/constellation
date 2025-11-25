use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Invalid state transition: {0}")]
    InvalidStateTransition(String),

    #[error("Log inconsistency: {0}")]
    LogInconsistency(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Not leader")]
    NotLeader,

    #[error("Timeout")]
    Timeout,

    #[error("Internal error: {0}")]
    Internal(String),
}
