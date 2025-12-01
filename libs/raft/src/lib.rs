//! Constellation Raft - A Raft consensus implementation
//!
//! This crate provides a generic Raft consensus algorithm implementation
//! that can be used with any storage backend and state machine.

mod config;
mod error;
mod log;
mod node;
mod rpc;
mod state;
mod state_machine;
mod storage;

pub use config::RaftConfig;
pub use error::{Error, Result};
pub use log::{LogEntry, LogIndex, Term};
pub use node::{ElectionResult, RaftNode, RaftNodeBuilder};
pub use rpc::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    RequestVoteRequest, RequestVoteResponse,
};
pub use state::State;
pub use state_machine::StateMachine;
pub use storage::{MemoryStorage, RaftStorage, Snapshot};
