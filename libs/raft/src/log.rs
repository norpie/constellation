use serde::{Deserialize, Serialize};

/// Term number in the Raft algorithm
///
/// Terms act as a logical clock. Each term begins with an election.
/// Terms are monotonically increasing.
pub type Term = u64;

/// Index into the log
///
/// Log indices are 1-indexed (0 means "no entry").
pub type LogIndex = u64;

/// A single entry in the replicated log
///
/// Each entry contains a command for the state machine and the term
/// when the entry was received by the leader.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogEntry {
    /// The term when this entry was created
    pub term: Term,

    /// The command to apply to the state machine
    ///
    /// This is opaque to the Raft algorithm - the state machine
    /// interprets it.
    pub command: Vec<u8>,
}

impl LogEntry {
    /// Create a new log entry
    pub fn new(term: Term, command: Vec<u8>) -> Self {
        Self { term, command }
    }
}
