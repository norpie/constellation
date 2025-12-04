//! Route resolution and peer discovery
//!
//! The Router provides context-aware route resolution with self-reference checking
//! and round-robin load balancing across nodes that handle the same route.

use crate::mesh::{AddressBook, TransponderData};
use constellation_raft::RaftNode;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use thiserror::Error;

/// Errors that can occur during route resolution
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum RoutingError {
    /// Route not found in address book
    #[error("Route not found: {0}")]
    RouteNotFound(String),

    /// Peer not found in address book
    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    /// Attempted to route to self
    #[error("Cannot route to self")]
    SelfReference,

    /// No usable address available for peer
    #[error("No address available for peer: {0}")]
    NoAddressAvailable(String),
}

/// A resolved connection target
///
/// Contains all information needed to establish a connection to a peer.
#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    /// The peer's node ID
    pub peer_id: String,
    /// The transport type (e.g., "tcp", "unix")
    pub transport: String,
    /// The connection address (e.g., "127.0.0.1:8080")
    pub address: String,
}

/// Route resolver with self-reference checking and load balancing
///
/// Router sits between the AddressBook (pure replicated data) and RpcClient
/// (RPC mechanics), providing:
/// - Self-reference checking (prevents calling yourself)
/// - Round-robin load balancing across nodes handling the same route
/// - Peer resolution for direct peer-to-peer communication
pub struct Router {
    /// This node's ID (for self-reference checking)
    self_id: String,
    /// Access to the address book via RaftNode
    raft: RaftNode<AddressBook>,
    /// Per-route round-robin state for load balancing
    rr_state: DashMap<String, AtomicUsize>,
}

impl Router {
    /// Create a new router
    pub fn new(self_id: String, raft: RaftNode<AddressBook>) -> Self {
        Self {
            self_id,
            raft,
            rr_state: DashMap::new(),
        }
    }

    /// Get peer's TransponderData
    ///
    /// Returns an error if the peer is not found or if attempting to look up self.
    pub async fn peer(&self, peer_id: &str) -> Result<TransponderData, RoutingError> {
        // Check for self-reference
        if peer_id == self.self_id {
            return Err(RoutingError::SelfReference);
        }

        // Look up peer in address book
        self.raft
            .with_state_machine(|address_book| {
                address_book
                    .get_node(peer_id)
                    .cloned()
                    .ok_or_else(|| RoutingError::PeerNotFound(peer_id.to_string()))
            })
            .await
    }

    /// Resolve a route to a connection target
    ///
    /// Uses round-robin selection across all nodes that handle the route.
    /// Returns an error if the route is not found or all nodes are self.
    pub async fn resolve_route(&self, route: &str) -> Result<ResolvedTarget, RoutingError> {
        // Get all nodes that handle this route
        let node_ids = self
            .raft
            .with_state_machine(|address_book| {
                address_book
                    .get_nodes_for_route(route)
                    .cloned()
                    .ok_or_else(|| RoutingError::RouteNotFound(route.to_string()))
            })
            .await?;

        // Filter out self
        let candidates: Vec<_> = node_ids
            .into_iter()
            .filter(|id| id != &self.self_id)
            .collect();

        if candidates.is_empty() {
            return Err(RoutingError::RouteNotFound(route.to_string()));
        }

        // Round-robin selection
        let idx = self
            .rr_state
            .entry(route.to_string())
            .or_insert_with(|| AtomicUsize::new(0))
            .fetch_add(1, Ordering::Relaxed)
            % candidates.len();

        let peer_id = &candidates[idx];

        // Resolve the selected peer to a target
        self.resolve_peer(peer_id).await
    }

    /// Resolve a specific peer to a connection target
    ///
    /// Returns an error if the peer is not found, is self, or has no usable address.
    pub async fn resolve_peer(&self, peer_id: &str) -> Result<ResolvedTarget, RoutingError> {
        // Get peer data (includes self-reference check)
        let transponder = self.peer(peer_id).await?;

        // Extract a usable address
        // For MVP: take first advertised address
        let advertised = transponder
            .addresses
            .first()
            .ok_or_else(|| RoutingError::NoAddressAvailable(peer_id.to_string()))?;

        Ok(ResolvedTarget {
            peer_id: peer_id.to_string(),
            transport: advertised.transport.clone(),
            address: advertised.address.clone(),
        })
    }

    /// Get any known peer ID
    ///
    /// Useful for bootstrap/join scenarios where you need to contact
    /// any member of the mesh to discover the rest.
    ///
    /// Returns `None` if no peers are known or all known nodes are self.
    pub async fn any_peer(&self) -> Option<String> {
        self.raft
            .with_state_machine(|address_book| {
                address_book
                    .all_nodes()
                    .keys()
                    .find(|id| *id != &self.self_id)
                    .cloned()
            })
            .await
    }

    /// Get the self node ID
    pub fn self_id(&self) -> &str {
        &self.self_id
    }
}

impl Clone for Router {
    fn clone(&self) -> Self {
        Self {
            self_id: self.self_id.clone(),
            raft: self.raft.clone(),
            rr_state: DashMap::new(), // Each clone gets fresh rr state
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests will be in libs/node/tests/router.rs for integration testing
}
