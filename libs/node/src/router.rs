//! Route resolution and peer discovery
//!
//! The Router provides context-aware route resolution with:
//! - Locality-based node ranking (same zone > same region > other)
//! - Constraint-based address filtering
//! - Self-reference checking (prevents calling yourself)

use crate::mesh::{AddressBook, AdvertisedAddress, Constraint, TransponderData};
use constellation_fabric::Codec;
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
    /// Available codecs (intersection of caller's and target's supported codecs)
    pub codecs: Vec<Codec>,
}

/// Compute locality score between caller and target
///
/// Lower score = better locality:
/// - 0: Same zone (best)
/// - 1: Same region, different zone
/// - 2: Different region
fn locality_score(
    caller_region: &str,
    caller_zone: &str,
    target_region: &str,
    target_zone: &str,
) -> u8 {
    if caller_zone == target_zone {
        0
    } else if caller_region == target_region {
        1
    } else {
        2
    }
}

/// Result of selecting an address from a target's advertised addresses
struct SelectedAddress {
    address: AdvertisedAddress,
    codecs: Vec<Codec>,
}

/// Select the best usable address from a target node
///
/// Filters addresses by both caller's and target's constraints,
/// and computes the codec intersection for each candidate.
/// Returns the first address that passes all checks.
fn select_address(
    caller: &TransponderData,
    target: &TransponderData,
) -> Option<SelectedAddress> {
    for addr in &target.addresses {
        // Check caller's constraints allow this network/transport
        if !caller
            .global_constraints
            .allows_transport(&addr.network, &addr.transport)
        {
            continue;
        }

        // Check target's constraints allow this network/transport
        if !target
            .global_constraints
            .allows_transport(&addr.network, &addr.transport)
        {
            continue;
        }

        // Compute codec intersection
        // Caller's allowed codecs for this network
        let caller_codecs = caller.global_constraints.rules_for(&addr.network);
        // Target's allowed codecs for this network
        let target_codecs = target.global_constraints.rules_for(&addr.network);

        // The address also advertises which codecs it supports
        let codecs: Vec<Codec> = addr
            .codecs
            .iter()
            .filter(|c| caller_codecs.allows_codec(c))
            .filter(|c| target_codecs.allows_codec(c))
            .cloned()
            .collect();

        // Need at least one codec in common
        if codecs.is_empty() {
            continue;
        }

        return Some(SelectedAddress {
            address: addr.clone(),
            codecs,
        });
    }

    None
}

/// Default caller data for bootstrap scenarios
///
/// When the caller isn't in the AddressBook yet (e.g., during bootstrap),
/// we use permissive defaults that allow any connection.
fn default_caller_data() -> TransponderData {
    TransponderData {
        node_id: String::new(),
        region: "global".to_string(),
        zone: "global".to_string(),
        addresses: vec![],
        transports: vec![],
        codecs: vec![],
        routes: vec![],
        global_constraints: Constraint::allow_all(),
        route_constraints: std::collections::HashMap::new(),
        capabilities: crate::mesh::Capabilities::basic(),
    }
}

/// Route resolver with locality ranking and constraint filtering
///
/// Router sits between the AddressBook (pure replicated data) and RpcClient
/// (RPC mechanics), providing:
/// - Locality-based node ranking (same zone > same region > other)
/// - Constraint-based address filtering (transport + codec restrictions)
/// - Self-reference checking (prevents calling yourself)
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
    /// Ranks candidate nodes by locality (same zone > same region > other),
    /// then tries each in order until finding one with a usable address.
    /// Uses round-robin within the same locality tier for load balancing.
    ///
    /// Returns an error if the route is not found or no node has a usable address.
    pub async fn resolve_route(&self, route: &str) -> Result<ResolvedTarget, RoutingError> {
        // Get all nodes that handle this route, along with their TransponderData
        let candidates: Vec<TransponderData> = self
            .raft
            .with_state_machine(|address_book| {
                let node_ids = address_book
                    .get_nodes_for_route(route)
                    .ok_or_else(|| RoutingError::RouteNotFound(route.to_string()))?;

                // Filter out self and collect TransponderData
                Ok(node_ids
                    .iter()
                    .filter(|id| *id != &self.self_id)
                    .filter_map(|id| address_book.get_node(id).cloned())
                    .collect::<Vec<_>>())
            })
            .await?;

        if candidates.is_empty() {
            return Err(RoutingError::RouteNotFound(route.to_string()));
        }

        // Get caller's region/zone for locality scoring
        let (caller_region, caller_zone) = self
            .raft
            .with_state_machine(|address_book| {
                address_book
                    .get_node(&self.self_id)
                    .map(|t| (t.region.clone(), t.zone.clone()))
                    .unwrap_or_else(|| ("global".to_string(), "global".to_string()))
            })
            .await;

        // Sort candidates by locality score (lower = better)
        let mut scored: Vec<(u8, &TransponderData)> = candidates
            .iter()
            .map(|t| {
                let score = locality_score(&caller_region, &caller_zone, &t.region, &t.zone);
                (score, t)
            })
            .collect();
        scored.sort_by_key(|(score, _)| *score);

        // Round-robin within same locality tier
        // Get the start index for this route
        let rr_idx = self
            .rr_state
            .entry(route.to_string())
            .or_insert_with(|| AtomicUsize::new(0))
            .fetch_add(1, Ordering::Relaxed);

        // Try each candidate in locality order, starting from rr_idx within each tier
        let mut current_score: Option<u8> = None;
        let mut tier_start = 0;
        let mut tier_len = 0;

        for (i, (score, _)) in scored.iter().enumerate() {
            // Track tier boundaries for round-robin
            if current_score != Some(*score) {
                current_score = Some(*score);
                tier_start = i;
                tier_len = scored.iter().skip(i).take_while(|(s, _)| s == score).count();
            }

            // Apply round-robin within this tier
            let tier_idx = (rr_idx + i - tier_start) % tier_len;
            let actual_idx = tier_start + tier_idx;

            if actual_idx < scored.len() {
                let (_, candidate) = &scored[actual_idx];
                match self.resolve_peer(&candidate.node_id).await {
                    Ok(target) => return Ok(target),
                    Err(RoutingError::NoAddressAvailable(_)) => continue,
                    Err(e) => return Err(e),
                }
            }
        }

        // All candidates tried, none had usable address
        Err(RoutingError::NoAddressAvailable(format!(
            "no usable address for route: {}",
            route
        )))
    }

    /// Resolve a specific peer to a connection target
    ///
    /// Filters the peer's addresses by both caller's and target's constraints,
    /// and computes the codec intersection for the selected address.
    ///
    /// Returns an error if the peer is not found, is self, or has no usable address.
    pub async fn resolve_peer(&self, peer_id: &str) -> Result<ResolvedTarget, RoutingError> {
        // Get peer data (includes self-reference check)
        let target = self.peer(peer_id).await?;

        // Get caller data (ourselves) - fallback to defaults if not in AddressBook yet
        let caller = self
            .raft
            .with_state_machine(|address_book| address_book.get_node(&self.self_id).cloned())
            .await
            .unwrap_or_else(default_caller_data);

        // Select an address that satisfies both parties' constraints
        let selected = select_address(&caller, &target)
            .ok_or_else(|| RoutingError::NoAddressAvailable(peer_id.to_string()))?;

        Ok(ResolvedTarget {
            peer_id: peer_id.to_string(),
            transport: selected.address.transport,
            address: selected.address.address,
            codecs: selected.codecs,
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
