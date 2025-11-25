use crate::Result;
use async_trait::async_trait;

/// State machine that processes committed log entries
///
/// This trait represents the application-specific logic that Raft replicates.
/// For example, in Constellation, the state machine is the AddressBook that
/// stores node locations and routes.
///
/// All methods must be deterministic - given the same sequence of commands,
/// all state machines must produce identical results.
#[async_trait]
pub trait StateMachine: Send {
    /// Apply a command to the state machine
    ///
    /// This is called in log order for each committed entry. The command
    /// bytes are opaque to Raft - the state machine interprets them.
    ///
    /// Returns the result of applying the command (e.g., success/error response
    /// that can be sent back to the client).
    async fn apply(&mut self, command: Vec<u8>) -> Result<Vec<u8>>;

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
