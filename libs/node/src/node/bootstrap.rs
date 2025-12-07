//! Cluster bootstrap and join logic

use crate::error::{Error, Result};
use crate::handler::Handler;
use std::collections::HashMap;

/// Build TransponderData for this node
pub(crate) fn build_self_transponder_data(
    node_id: &str,
    region: &str,
    zone: &str,
    advertise_addresses: &[crate::mesh::AddressGroup],
    routes: &HashMap<String, &'static dyn Handler>,
    global_constraints: &crate::mesh::Constraint,
) -> crate::mesh::TransponderData {
    let route_names: Vec<String> = routes.keys().cloned().collect();

    // Collect unique transports
    let transports: Vec<String> = advertise_addresses
        .iter()
        .map(|a| a.transport.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    crate::mesh::TransponderData::builder()
        .node_id(node_id)
        .region(region)
        .zone(zone)
        .addresses(advertise_addresses.to_vec())
        .transports(transports)
        .codec("bincode") // Currently hardcoded
        .routes(route_names)
        .global_constraints(global_constraints.clone())
        .capabilities(crate::mesh::Capabilities::basic())
        .build()
}

/// Attempt to join the cluster via bootstrap peers
///
/// Returns Ok(()) if successfully joined or formed new cluster.
/// Returns Err if unable to join and unable to form new cluster.
pub(crate) async fn bootstrap_join(
    bootstrap_peers: &[(String, crate::mesh::AddressGroup)],
    self_data: &crate::mesh::TransponderData,
    raft: &constellation_raft::RaftNode<crate::mesh::AddressBook>,
    can_lead: bool,
) -> Result<()> {
    println!("[bootstrap_join] Node {} starting bootstrap (peers: {})", self_data.node_id, bootstrap_peers.len());

    // If no bootstrap peers, we're forming a new cluster
    if bootstrap_peers.is_empty() {
        println!("[bootstrap_join] No bootstrap peers - forming new cluster");
        if !can_lead {
            return Err(Error::Custom(
                "Cannot start: no bootstrap peers and can_lead=false".to_string(),
            ));
        }
        // First node: become leader first, then add ourselves via the log
        // This ensures our Join entry is in the log and will be replicated to joiners.
        raft.start_election().await?;
        println!("[bootstrap_join] First node became leader");

        // Now submit our Join command through the log
        let command = crate::mesh::AddressBookCommand::Join(self_data.clone());
        let bytes = constellation_fabric::Codec::Bincode
            .encode(&command)
            .map_err(|e| Error::Custom(format!("Failed to serialize join command: {}", e)))?;
        raft.submit_command(bytes).await?;
        println!("[bootstrap_join] First node added self to AddressBook");
        return Ok(());
    }

    // Try bootstrap peers sequentially
    for (peer_id, advertised) in bootstrap_peers {
        let address = &advertised.address;

        println!("[bootstrap_join] Trying to join via peer {} at {}", peer_id, address);

        // Attempt join
        match try_join(address, self_data).await {
            Ok(crate::mesh::MeshResponse::Success) => {
                println!("[bootstrap_join] Successfully joined via {}", address);
                return Ok(());
            }
            Ok(crate::mesh::MeshResponse::NotLeader {
                leader: Some(leader_data),
            }) => {
                println!("[bootstrap_join] Peer {} is not leader, redirecting to {:?}", address, leader_data.node_id);
                // Got redirected to leader, try that
                if let Some(leader_addr) = leader_data.addresses.first().map(|a| &a.address) {
                    println!("[bootstrap_join] Trying leader at {}", leader_addr);
                    if let Ok(crate::mesh::MeshResponse::Success) =
                        try_join(leader_addr, self_data).await
                    {
                        println!("[bootstrap_join] Successfully joined via leader");
                        return Ok(());
                    }
                }
            }
            Ok(crate::mesh::MeshResponse::NotLeader { leader: None }) => {
                println!("[bootstrap_join] Peer {} has no leader info, trying next", address);
                continue;
            }
            Err(e) => {
                println!("[bootstrap_join] Connection to {} failed: {}", address, e);
                continue;
            }
        }
    }

    println!("[bootstrap_join] Failed to join via any bootstrap peer");
    Err(Error::Custom(
        "Failed to join cluster via any bootstrap peer".to_string(),
    ))
}

async fn try_join(
    address: &str,
    self_data: &crate::mesh::TransponderData,
) -> Result<crate::mesh::MeshResponse> {
    println!("[try_join] Sending _mesh.join to {}", address);
    let result = crate::rpc::send_direct(address, "_mesh.join", self_data).await;
    println!("[try_join] Result: {:?}", result.as_ref().map(|_| "ok").map_err(|e| e.to_string()));
    result
}
