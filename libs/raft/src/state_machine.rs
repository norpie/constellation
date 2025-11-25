use crate::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// State machine that processes committed log entries
///
/// This trait represents the application-specific logic that Raft replicates.
/// For example, in Constellation, the state machine is the AddressBook that
/// stores node locations and routes.
///
/// All methods must be deterministic - given the same sequence of commands,
/// all state machines must produce identical results.
///
/// The associated types allow strongly-typed commands and responses while
/// keeping the encoding/decoding logic in the Raft layer.
#[async_trait]
pub trait StateMachine: Send {
    /// The command type this state machine accepts
    ///
    /// Must be serializable so Raft can store it in the log.
    type Command: Serialize + for<'de> Deserialize<'de> + Send;

    /// The response type this state machine produces
    ///
    /// Must be serializable so it can be sent back to clients.
    type Response: Serialize + for<'de> Deserialize<'de> + Send;

    /// Apply a command to the state machine
    ///
    /// This is called in log order for each committed entry. The command
    /// is already deserialized by Raft.
    ///
    /// Returns the result of applying the command (e.g., success/error response
    /// that can be sent back to the client).
    async fn apply(&mut self, command: Self::Command) -> Result<Self::Response>;

    /// Create a snapshot of the current state
    ///
    /// Used for log compaction. The snapshot should capture the complete
    /// state so that the log can be truncated.
    async fn snapshot(&self) -> Result<Vec<u8>>;

    /// Restore state from a snapshot
    ///
    /// Used when installing snapshots from the leader or during startup.
    async fn restore(&mut self, snapshot: Vec<u8>) -> Result<()>;
}
