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
}
