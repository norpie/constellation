use crate::{Error, LogIndex, RaftStorage, Result, State, StateMachine, Term};
use constellation_fabric::codec::BincodeCodec;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A Raft consensus node
///
/// This is the main entry point for the Raft algorithm. It manages elections,
/// log replication, and state machine application.
///
/// The node is generic over the state machine type, allowing any application-specific
/// logic to be replicated via Raft consensus.
pub struct RaftNode<SM: StateMachine> {
    inner: Arc<RwLock<RaftNodeInner<SM>>>,
}

impl<SM: StateMachine> Clone for RaftNode<SM> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct RaftNodeInner<SM: StateMachine> {
    // Identity
    node_id: String,
    can_lead: bool,
    peers: Vec<String>,

    // Storage and state machine
    storage: Box<dyn RaftStorage>,
    state_machine: SM,
    codec: BincodeCodec,

    // Volatile state (all servers)
    state: State,
    commit_index: LogIndex,
    last_applied: LogIndex,
    current_leader: Option<String>,

    // Volatile state (leaders only)
    // Reinitialized after each election
    next_index: HashMap<String, LogIndex>,
    match_index: HashMap<String, LogIndex>,

    // Election state
    votes_received: HashSet<String>,
}

impl<SM: StateMachine> RaftNode<SM> {
    /// Create a new builder for RaftNode
    pub fn builder() -> RaftNodeBuilder<SM> {
        RaftNodeBuilder::new()
    }

    /// Get the current node ID
    pub async fn node_id(&self) -> String {
        let inner = self.inner.read().await;
        inner.node_id.clone()
    }

    /// Check if this node can become a leader
    pub async fn can_lead(&self) -> bool {
        let inner = self.inner.read().await;
        inner.can_lead
    }

    /// Get the current state (Follower, Candidate, or Leader)
    pub async fn state(&self) -> State {
        let inner = self.inner.read().await;
        inner.state
    }

    /// Get the current term from storage
    pub async fn current_term(&self) -> Result<Term> {
        let inner = self.inner.read().await;
        inner.storage.get_term().await
    }

    /// Get the current commit index
    pub async fn commit_index(&self) -> LogIndex {
        let inner = self.inner.read().await;
        inner.commit_index
    }

    /// Get the last applied index
    pub async fn last_applied(&self) -> LogIndex {
        let inner = self.inner.read().await;
        inner.last_applied
    }

    /// Get the current leader (if known)
    pub async fn current_leader(&self) -> Option<String> {
        let inner = self.inner.read().await;
        inner.current_leader.clone()
    }

    /// Check if this node is the current leader
    pub async fn is_leader(&self) -> bool {
        let inner = self.inner.read().await;
        inner.state.is_leader()
    }

    /// Get the list of peers
    pub async fn peers(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner.peers.clone()
    }

    // State transition methods

    /// Convert to follower state
    ///
    /// This is called when:
    /// - Receiving an RPC with a higher term
    /// - Discovering a valid leader
    /// - Starting up
    async fn convert_to_follower(&self, term: Term) -> Result<()> {
        let mut inner = self.inner.write().await;

        // Update term if higher
        if term > inner.storage.get_term().await? {
            inner.storage.save_term(term).await?;
            inner.storage.save_voted_for(None).await?;
        }

        inner.state = State::Follower;
        inner.current_leader = None;
        inner.votes_received.clear();

        Ok(())
    }

    /// Convert to candidate state and start election
    ///
    /// This is called when election timeout elapses and can_lead is true.
    async fn convert_to_candidate(&self) -> Result<()> {
        let mut inner = self.inner.write().await;

        // Increment term
        let new_term = inner.storage.get_term().await? + 1;
        inner.storage.save_term(new_term).await?;

        // Vote for self
        let node_id = inner.node_id.clone();
        inner.storage.save_voted_for(Some(node_id.clone())).await?;

        inner.state = State::Candidate;
        inner.votes_received.clear();
        inner.votes_received.insert(node_id);

        Ok(())
    }

    /// Convert to leader state
    ///
    /// This is called when receiving majority votes as a candidate.
    async fn convert_to_leader(&self) -> Result<()> {
        let mut inner = self.inner.write().await;

        inner.state = State::Leader;
        inner.current_leader = Some(inner.node_id.clone());

        // Initialize leader volatile state
        // next_index: for each server, index of next log entry to send
        // (initialized to leader's last log index + 1)
        let last_log_index = inner.storage.last_log_index().await?;
        inner.next_index.clear();
        inner.match_index.clear();

        let peers = inner.peers.clone();
        for peer in peers {
            inner.next_index.insert(peer.clone(), last_log_index + 1);
            inner.match_index.insert(peer, 0);
        }

        Ok(())
    }

    // Helper methods

    /// Calculate the majority size for the cluster
    fn calculate_majority(cluster_size: usize) -> usize {
        cluster_size / 2 + 1
    }

    /// Check if we have received votes from a majority
    pub async fn has_majority_votes(&self) -> bool {
        let inner = self.inner.read().await;
        let cluster_size = inner.peers.len() + 1; // peers + self
        let majority = Self::calculate_majority(cluster_size);
        inner.votes_received.len() >= majority
    }

    /// Check if a candidate's log is at least as up-to-date as ours
    ///
    /// Comparison rule from Raft paper:
    /// - If terms differ, the log with the later term is more up-to-date
    /// - If terms are the same, the longer log is more up-to-date
    async fn is_log_up_to_date(&self, last_log_term: Term, last_log_index: LogIndex) -> Result<bool> {
        let inner = self.inner.read().await;

        let our_last_term = inner.storage.last_log_term().await?;
        let our_last_index = inner.storage.last_log_index().await?;

        // Compare terms first
        if last_log_term != our_last_term {
            return Ok(last_log_term > our_last_term);
        }

        // Terms are equal, compare lengths
        Ok(last_log_index >= our_last_index)
    }

    // RPC Handlers

    /// Handle incoming RequestVote RPC
    ///
    /// Invoked by candidates to gather votes during elections.
    pub async fn handle_request_vote(
        &self,
        request: crate::RequestVoteRequest,
    ) -> Result<crate::RequestVoteResponse> {
        let mut inner = self.inner.write().await;

        let current_term = inner.storage.get_term().await?;

        // Rule 1: Reply false if term < currentTerm
        if request.term < current_term {
            return Ok(crate::RequestVoteResponse {
                term: current_term,
                vote_granted: false,
            });
        }

        // If RPC request contains term > currentTerm: update term and convert to follower
        if request.term > current_term {
            inner.storage.save_term(request.term).await?;
            inner.storage.save_voted_for(None).await?;
            inner.state = State::Follower;
            inner.current_leader = None;
            inner.votes_received.clear();
        }

        let voted_for = inner.storage.get_voted_for().await?;

        // Rule 2: If votedFor is null or candidateId, and candidate's log is
        // at least as up-to-date as receiver's log, grant vote
        let can_vote = voted_for.is_none() || voted_for.as_deref() == Some(&request.candidate_id);

        if !can_vote {
            return Ok(crate::RequestVoteResponse {
                term: request.term,
                vote_granted: false,
            });
        }

        // Check if candidate's log is up-to-date
        let our_last_term = inner.storage.last_log_term().await?;
        let our_last_index = inner.storage.last_log_index().await?;

        let log_is_up_to_date = if request.last_log_term != our_last_term {
            request.last_log_term > our_last_term
        } else {
            request.last_log_index >= our_last_index
        };

        if !log_is_up_to_date {
            return Ok(crate::RequestVoteResponse {
                term: request.term,
                vote_granted: false,
            });
        }

        // Grant vote
        inner.storage.save_voted_for(Some(request.candidate_id.clone())).await?;

        Ok(crate::RequestVoteResponse {
            term: request.term,
            vote_granted: true,
        })
    }

    /// Handle incoming AppendEntries RPC
    ///
    /// Used for both log replication and heartbeats (empty entries).
    pub async fn handle_append_entries(
        &self,
        request: crate::AppendEntriesRequest,
    ) -> Result<crate::AppendEntriesResponse> {
        let mut inner = self.inner.write().await;

        let current_term = inner.storage.get_term().await?;

        // Rule 1: Reply false if term < currentTerm
        if request.term < current_term {
            return Ok(crate::AppendEntriesResponse {
                term: current_term,
                success: false,
                conflict_term: None,
                conflict_index: None,
            });
        }

        // If RPC contains term >= currentTerm: convert to follower and update term
        if request.term >= current_term {
            if request.term > current_term {
                inner.storage.save_term(request.term).await?;
                inner.storage.save_voted_for(None).await?;
            }
            inner.state = State::Follower;
            inner.current_leader = Some(request.leader_id.clone());
            inner.votes_received.clear();
        }

        // Rule 2: Reply false if log doesn't contain entry at prev_log_index
        // with matching prev_log_term
        if request.prev_log_index > 0 {
            let our_log_len = inner.storage.last_log_index().await?;

            // We don't have an entry at prev_log_index
            if request.prev_log_index > our_log_len {
                return Ok(crate::AppendEntriesResponse {
                    term: request.term,
                    success: false,
                    conflict_term: None,
                    conflict_index: Some(our_log_len + 1),
                });
            }

            // We have an entry at prev_log_index, check term matches
            let prev_entry = inner.storage.get_entry(request.prev_log_index).await?;
            if let Some(entry) = prev_entry {
                if entry.term != request.prev_log_term {
                    // Find the first index of the conflicting term
                    let conflict_term = entry.term;
                    let mut conflict_index = request.prev_log_index;

                    // Search backwards to find first entry of this term
                    while conflict_index > 1 {
                        if let Some(e) = inner.storage.get_entry(conflict_index - 1).await? {
                            if e.term != conflict_term {
                                break;
                            }
                            conflict_index -= 1;
                        } else {
                            break;
                        }
                    }

                    return Ok(crate::AppendEntriesResponse {
                        term: request.term,
                        success: false,
                        conflict_term: Some(conflict_term),
                        conflict_index: Some(conflict_index),
                    });
                }
            } else {
                // Entry doesn't exist (shouldn't happen if prev_log_index <= our_log_len)
                return Ok(crate::AppendEntriesResponse {
                    term: request.term,
                    success: false,
                    conflict_term: None,
                    conflict_index: Some(request.prev_log_index),
                });
            }
        }

        // Rule 3: If an existing entry conflicts with a new one (same index,
        // different terms), delete the existing entry and all that follow it
        if !request.entries.is_empty() {
            let mut index = request.prev_log_index + 1;

            for (i, new_entry) in request.entries.iter().enumerate() {
                if let Some(existing_entry) = inner.storage.get_entry(index).await? {
                    // Conflict: same index, different term
                    if existing_entry.term != new_entry.term {
                        // Delete this entry and all following
                        inner.storage.delete_entries_from(index).await?;
                        // Append remaining new entries
                        inner.storage.append_entries(request.entries[i..].to_vec()).await?;
                        break;
                    }
                } else {
                    // No existing entry, append all remaining new entries
                    inner.storage.append_entries(request.entries[i..].to_vec()).await?;
                    break;
                }
                index += 1;
            }
        }

        // Rule 5: If leaderCommit > commitIndex, set commitIndex =
        // min(leaderCommit, index of last new entry)
        if request.leader_commit > inner.commit_index {
            let last_new_entry_index = if request.entries.is_empty() {
                request.prev_log_index
            } else {
                request.prev_log_index + request.entries.len() as u64
            };

            inner.commit_index = request.leader_commit.min(last_new_entry_index);
        }

        Ok(crate::AppendEntriesResponse {
            term: request.term,
            success: true,
            conflict_term: None,
            conflict_index: None,
        })
    }
}

/// Builder for RaftNode
pub struct RaftNodeBuilder<SM: StateMachine> {
    node_id: Option<String>,
    can_lead: bool,
    peers: Vec<String>,
    storage: Option<Box<dyn RaftStorage>>,
    state_machine: Option<SM>,
}

impl<SM: StateMachine> Default for RaftNodeBuilder<SM> {
    fn default() -> Self {
        Self::new()
    }
}

impl<SM: StateMachine> RaftNodeBuilder<SM> {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            node_id: None,
            can_lead: true,
            peers: Vec::new(),
            storage: None,
            state_machine: None,
        }
    }

    /// Set the node ID
    ///
    /// This must be unique across the cluster.
    pub fn node_id(mut self, id: impl Into<String>) -> Self {
        self.node_id = Some(id.into());
        self
    }

    /// Set whether this node can become a leader
    ///
    /// - `true` (default): Node can start elections and become leader
    /// - `false`: Node never starts elections (remains follower), but still votes and counts toward quorum
    pub fn can_lead(mut self, can_lead: bool) -> Self {
        self.can_lead = can_lead;
        self
    }

    /// Set the list of peer node IDs
    ///
    /// This should include all other voting members of the cluster.
    pub fn peers(mut self, peers: Vec<String>) -> Self {
        self.peers = peers;
        self
    }

    /// Add a single peer node ID
    pub fn peer(mut self, peer: impl Into<String>) -> Self {
        self.peers.push(peer.into());
        self
    }

    /// Set the storage backend
    ///
    /// Defaults to MemoryStorage if not provided.
    pub fn storage<S: RaftStorage + 'static>(mut self, storage: S) -> Self {
        self.storage = Some(Box::new(storage));
        self
    }

    /// Set the state machine
    ///
    /// This is required.
    pub fn state_machine(mut self, state_machine: SM) -> Self {
        self.state_machine = Some(state_machine);
        self
    }

    /// Build the RaftNode
    pub fn build(self) -> Result<RaftNode<SM>> {
        let node_id = self
            .node_id
            .ok_or_else(|| Error::Internal("node_id is required".to_string()))?;

        let state_machine = self
            .state_machine
            .ok_or_else(|| Error::Internal("state_machine is required".to_string()))?;

        let storage = self
            .storage
            .unwrap_or_else(|| Box::new(crate::MemoryStorage::new()));

        let inner = RaftNodeInner {
            node_id,
            can_lead: self.can_lead,
            peers: self.peers,
            storage,
            state_machine,
            codec: BincodeCodec,
            state: State::Follower,
            commit_index: 0,
            last_applied: 0,
            current_leader: None,
            next_index: HashMap::new(),
            match_index: HashMap::new(),
            votes_received: HashSet::new(),
        };

        Ok(RaftNode {
            inner: Arc::new(RwLock::new(inner)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    // Simple test state machine
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    struct TestCommand;

    #[derive(Debug, Serialize, Deserialize)]
    struct TestResponse;

    struct TestStateMachine;

    #[async_trait::async_trait]
    impl StateMachine for TestStateMachine {
        type Command = TestCommand;
        type Response = TestResponse;

        async fn apply(&mut self, _command: Self::Command) -> Result<Self::Response> {
            Ok(TestResponse)
        }

        async fn snapshot(&self) -> Result<Vec<u8>> {
            Ok(vec![])
        }

        async fn restore(&mut self, _snapshot: Vec<u8>) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_raft_node_builder() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .can_lead(true)
            .peers(vec!["node-2".to_string(), "node-3".to_string()])
            .storage(MemoryStorage::new())
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        assert_eq!(node.node_id().await, "node-1");
        assert!(node.can_lead().await);
        assert_eq!(node.peers().await, vec!["node-2", "node-3"]);
        assert_eq!(node.state().await, State::Follower);
        assert_eq!(node.commit_index().await, 0);
        assert_eq!(node.last_applied().await, 0);
        assert_eq!(node.current_leader().await, None);
    }

    #[tokio::test]
    async fn test_raft_node_builder_defaults() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        assert!(node.can_lead().await); // Defaults to true
        assert_eq!(node.peers().await.len(), 0); // No peers
    }

    #[tokio::test]
    async fn test_raft_node_builder_missing_required() {
        // Missing node_id
        let result = RaftNode::<TestStateMachine>::builder()
            .state_machine(TestStateMachine)
            .build();
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("node_id"));
        }

        // Missing state_machine
        let result = RaftNode::<TestStateMachine>::builder()
            .node_id("node-1")
            .build();
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("state_machine"));
        }
    }

    #[tokio::test]
    async fn test_raft_node_can_lead_false() {
        let node = RaftNode::builder()
            .node_id("observer")
            .can_lead(false)
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        assert!(!node.can_lead().await);
    }

    #[tokio::test]
    async fn test_raft_node_clone() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        let cloned = node.clone();
        assert_eq!(cloned.node_id().await, "node-1");
    }

    #[tokio::test]
    async fn test_convert_to_follower() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Start as follower in term 0
        assert_eq!(node.state().await, State::Follower);
        assert_eq!(node.current_term().await.unwrap(), 0);

        // Convert to follower with higher term
        node.convert_to_follower(5).await.unwrap();
        assert_eq!(node.state().await, State::Follower);
        assert_eq!(node.current_term().await.unwrap(), 5);
        assert_eq!(node.current_leader().await, None);
    }

    #[tokio::test]
    async fn test_convert_to_candidate() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .peers(vec!["node-2".to_string(), "node-3".to_string()])
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Convert to candidate
        node.convert_to_candidate().await.unwrap();

        assert_eq!(node.state().await, State::Candidate);
        assert_eq!(node.current_term().await.unwrap(), 1); // Incremented from 0
        assert!(node.has_majority_votes().await == false); // Only voted for self (1/3)
    }

    #[tokio::test]
    async fn test_convert_to_leader() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .peers(vec!["node-2".to_string(), "node-3".to_string()])
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // First become candidate
        node.convert_to_candidate().await.unwrap();

        // Then become leader
        node.convert_to_leader().await.unwrap();

        assert_eq!(node.state().await, State::Leader);
        assert!(node.is_leader().await);
        assert_eq!(node.current_leader().await, Some("node-1".to_string()));
    }

    #[tokio::test]
    async fn test_majority_calculation() {
        // 3 node cluster: need 2 votes
        assert_eq!(RaftNode::<TestStateMachine>::calculate_majority(3), 2);

        // 5 node cluster: need 3 votes
        assert_eq!(RaftNode::<TestStateMachine>::calculate_majority(5), 3);

        // 1 node cluster: need 1 vote
        assert_eq!(RaftNode::<TestStateMachine>::calculate_majority(1), 1);
    }

    #[tokio::test]
    async fn test_request_vote_grant() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .peers(vec!["node-2".to_string()])
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Request vote from node-2 in term 1
        let request = crate::RequestVoteRequest {
            term: 1,
            candidate_id: "node-2".to_string(),
            last_log_index: 0,
            last_log_term: 0,
        };

        let response = node.handle_request_vote(request).await.unwrap();

        assert_eq!(response.term, 1);
        assert!(response.vote_granted);

        // Verify we updated our term
        assert_eq!(node.current_term().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_request_vote_reject_old_term() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // First, advance to term 5
        node.convert_to_follower(5).await.unwrap();

        // Request vote in old term 3
        let request = crate::RequestVoteRequest {
            term: 3,
            candidate_id: "node-2".to_string(),
            last_log_index: 0,
            last_log_term: 0,
        };

        let response = node.handle_request_vote(request).await.unwrap();

        assert_eq!(response.term, 5);
        assert!(!response.vote_granted);
    }

    #[tokio::test]
    async fn test_request_vote_reject_already_voted() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Vote for node-2 in term 1
        let request1 = crate::RequestVoteRequest {
            term: 1,
            candidate_id: "node-2".to_string(),
            last_log_index: 0,
            last_log_term: 0,
        };

        let response1 = node.handle_request_vote(request1).await.unwrap();
        assert!(response1.vote_granted);

        // node-3 tries to get our vote in same term
        let request2 = crate::RequestVoteRequest {
            term: 1,
            candidate_id: "node-3".to_string(),
            last_log_index: 0,
            last_log_term: 0,
        };

        let response2 = node.handle_request_vote(request2).await.unwrap();
        assert!(!response2.vote_granted); // Already voted for node-2
    }

    #[tokio::test]
    async fn test_request_vote_reject_stale_log() {
        let mut storage = MemoryStorage::new();

        // Add some log entries (term 2, 3 entries)
        storage.save_term(2).await.unwrap();
        storage
            .append_entries(vec![
                crate::LogEntry::new(2, vec![1]),
                crate::LogEntry::new(2, vec![2]),
                crate::LogEntry::new(2, vec![3]),
            ])
            .await
            .unwrap();

        let node = RaftNode::builder()
            .node_id("node-1")
            .storage(storage)
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Candidate with shorter log tries to get vote
        let request = crate::RequestVoteRequest {
            term: 3,
            candidate_id: "node-2".to_string(),
            last_log_index: 1, // We have 3 entries
            last_log_term: 2,
        };

        let response = node.handle_request_vote(request).await.unwrap();
        assert!(!response.vote_granted); // Log not up-to-date
    }

    #[tokio::test]
    async fn test_append_entries_heartbeat() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Heartbeat from leader (empty entries)
        let request = crate::AppendEntriesRequest {
            term: 1,
            leader_id: "leader".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![],
            leader_commit: 0,
        };

        let response = node.handle_append_entries(request).await.unwrap();

        assert_eq!(response.term, 1);
        assert!(response.success);
        assert_eq!(node.current_term().await.unwrap(), 1);
        assert_eq!(node.current_leader().await, Some("leader".to_string()));
        assert_eq!(node.state().await, State::Follower);
    }

    #[tokio::test]
    async fn test_append_entries_success() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Append some entries
        let request = crate::AppendEntriesRequest {
            term: 1,
            leader_id: "leader".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![
                crate::LogEntry::new(1, vec![1]),
                crate::LogEntry::new(1, vec![2]),
            ],
            leader_commit: 0,
        };

        let response = node.handle_append_entries(request).await.unwrap();

        assert!(response.success);

        // Verify entries were appended
        let inner = node.inner.read().await;
        assert_eq!(inner.storage.last_log_index().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_append_entries_reject_old_term() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Advance to term 5
        node.convert_to_follower(5).await.unwrap();

        // Receive AppendEntries from old term
        let request = crate::AppendEntriesRequest {
            term: 3,
            leader_id: "old-leader".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![],
            leader_commit: 0,
        };

        let response = node.handle_append_entries(request).await.unwrap();

        assert!(!response.success);
        assert_eq!(response.term, 5);
    }

    #[tokio::test]
    async fn test_append_entries_reject_missing_prev() {
        let mut storage = MemoryStorage::new();

        // We have 2 entries
        storage
            .append_entries(vec![
                crate::LogEntry::new(1, vec![1]),
                crate::LogEntry::new(1, vec![2]),
            ])
            .await
            .unwrap();

        let node = RaftNode::builder()
            .node_id("node-1")
            .storage(storage)
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Leader thinks we have entry at index 5
        let request = crate::AppendEntriesRequest {
            term: 1,
            leader_id: "leader".to_string(),
            prev_log_index: 5,
            prev_log_term: 1,
            entries: vec![crate::LogEntry::new(1, vec![99])],
            leader_commit: 0,
        };

        let response = node.handle_append_entries(request).await.unwrap();

        assert!(!response.success);
        assert_eq!(response.conflict_index, Some(3)); // We have 2, next would be 3
    }

    #[tokio::test]
    async fn test_append_entries_reject_term_mismatch() {
        let mut storage = MemoryStorage::new();

        // We have entries from term 1
        storage.save_term(1).await.unwrap();
        storage
            .append_entries(vec![
                crate::LogEntry::new(1, vec![1]),
                crate::LogEntry::new(1, vec![2]),
            ])
            .await
            .unwrap();

        let node = RaftNode::builder()
            .node_id("node-1")
            .storage(storage)
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Leader thinks index 2 has term 2 (but we have term 1)
        let request = crate::AppendEntriesRequest {
            term: 2,
            leader_id: "leader".to_string(),
            prev_log_index: 2,
            prev_log_term: 2, // Wrong! Our entry at index 2 has term 1
            entries: vec![crate::LogEntry::new(2, vec![99])],
            leader_commit: 0,
        };

        let response = node.handle_append_entries(request).await.unwrap();

        assert!(!response.success);
        assert_eq!(response.conflict_term, Some(1));
        assert!(response.conflict_index.is_some());
    }

    #[tokio::test]
    async fn test_append_entries_commit_advance() {
        let node = RaftNode::builder()
            .node_id("node-1")
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Leader sends entries with commit=2
        let request = crate::AppendEntriesRequest {
            term: 1,
            leader_id: "leader".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![
                crate::LogEntry::new(1, vec![1]),
                crate::LogEntry::new(1, vec![2]),
                crate::LogEntry::new(1, vec![3]),
            ],
            leader_commit: 2,
        };

        node.handle_append_entries(request).await.unwrap();

        // Commit index should advance to 2
        assert_eq!(node.commit_index().await, 2);
    }

    #[tokio::test]
    async fn test_append_entries_conflict_resolution() {
        let mut storage = MemoryStorage::new();

        // We have conflicting entries from a split brain scenario
        storage.save_term(2).await.unwrap();
        storage
            .append_entries(vec![
                crate::LogEntry::new(1, vec![1]),
                crate::LogEntry::new(2, vec![99]), // Conflict at index 2
                crate::LogEntry::new(2, vec![98]), // This should be deleted
            ])
            .await
            .unwrap();

        let node = RaftNode::builder()
            .node_id("node-1")
            .storage(storage)
            .state_machine(TestStateMachine)
            .build()
            .unwrap();

        // Leader sends correct entries
        let request = crate::AppendEntriesRequest {
            term: 3,
            leader_id: "leader".to_string(),
            prev_log_index: 1,
            prev_log_term: 1,
            entries: vec![
                crate::LogEntry::new(3, vec![2]), // Replaces our term 2 entry
                crate::LogEntry::new(3, vec![3]),
            ],
            leader_commit: 0,
        };

        let response = node.handle_append_entries(request).await.unwrap();
        assert!(response.success);

        // Verify log was corrected
        let inner = node.inner.read().await;
        assert_eq!(inner.storage.last_log_index().await.unwrap(), 3);
        let entry2 = inner.storage.get_entry(2).await.unwrap().unwrap();
        assert_eq!(entry2.term, 3);
        assert_eq!(entry2.command, vec![2]);
    }
}
