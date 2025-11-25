/// Server state in the Raft cluster
///
/// Every server is always in exactly one of these three states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Passive; responds to RPCs from leaders and candidates.
    /// If no communication received within election timeout, becomes candidate.
    Follower,

    /// Actively seeking votes to become leader.
    /// Transitions to leader upon majority vote, back to follower if another
    /// leader is discovered, or restarts election on timeout.
    Candidate,

    /// Handles all client requests, replicates log entries to followers,
    /// sends heartbeats. Only one leader per term.
    Leader,
}

impl State {
    /// Returns true if this state is Follower
    pub fn is_follower(&self) -> bool {
        matches!(self, State::Follower)
    }

    /// Returns true if this state is Candidate
    pub fn is_candidate(&self) -> bool {
        matches!(self, State::Candidate)
    }

    /// Returns true if this state is Leader
    pub fn is_leader(&self) -> bool {
        matches!(self, State::Leader)
    }
}
