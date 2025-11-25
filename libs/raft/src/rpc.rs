use crate::{LogEntry, LogIndex, Term};
use serde::{Deserialize, Serialize};

/// RequestVote RPC - Invoked by candidates to gather votes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteRequest {
    /// Candidate's term
    pub term: Term,

    /// Candidate requesting vote
    pub candidate_id: String,

    /// Index of candidate's last log entry
    pub last_log_index: LogIndex,

    /// Term of candidate's last log entry
    pub last_log_term: Term,
}

/// RequestVote RPC response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteResponse {
    /// Current term, for candidate to update itself
    pub term: Term,

    /// True means candidate received vote
    pub vote_granted: bool,
}

/// AppendEntries RPC - Used for log replication and heartbeats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesRequest {
    /// Leader's term
    pub term: Term,

    /// Leader's ID, so follower can redirect clients
    pub leader_id: String,

    /// Index of log entry immediately preceding new ones
    pub prev_log_index: LogIndex,

    /// Term of prev_log_index entry
    pub prev_log_term: Term,

    /// Log entries to store (empty for heartbeat)
    pub entries: Vec<LogEntry>,

    /// Leader's commit index
    pub leader_commit: LogIndex,
}

/// AppendEntries RPC response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesResponse {
    /// Current term, for leader to update itself
    pub term: Term,

    /// True if follower contained entry matching prev_log_index and prev_log_term
    pub success: bool,

    /// Optimization: when rejecting, include conflicting term and first index
    /// This allows leader to skip all entries in that term at once
    pub conflict_term: Option<Term>,
    pub conflict_index: Option<LogIndex>,
}

/// InstallSnapshot RPC - Used to send snapshot chunks to slow followers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallSnapshotRequest {
    /// Leader's term
    pub term: Term,

    /// Leader's ID, for follower redirection
    pub leader_id: String,

    /// Snapshot replaces all entries up through this index
    pub last_included_index: LogIndex,

    /// Term of last_included_index
    pub last_included_term: Term,

    /// Byte offset in the snapshot file
    pub offset: u64,

    /// Raw bytes of the snapshot chunk
    pub data: Vec<u8>,

    /// True if this is the last chunk
    pub done: bool,
}

/// InstallSnapshot RPC response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallSnapshotResponse {
    /// Current term, for leader to update itself
    pub term: Term,
}
