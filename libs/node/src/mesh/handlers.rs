//! Built-in handlers for mesh operations

use crate::handler;
use crate::mesh::{AddressBook, AddressBookCommand, LeaveRequest, MeshResponse, TransponderData};
use crate::raft::RAFT_LOG_CODEC;
use crate::Data;
use constellation_raft::RaftNode;

/// Built-in handler for mesh join requests
///
/// Allows new nodes to join the cluster. If this node is the leader,
/// it submits a Join command to Raft. Otherwise, it returns information
/// about the current leader so the client can retry there.
#[handler(route = "_mesh.join")]
async fn mesh_join(
    req: TransponderData,
    raft: Data<RaftNode<AddressBook>>,
) -> Result<MeshResponse, crate::error::Error> {
    println!("[_mesh.join] Received join request from node: {}", req.node_id);

    // Check if we're the leader
    let is_leader = raft.is_leader().await;
    println!("[_mesh.join] Am I leader? {}", is_leader);

    if !is_leader {
        // Get leader info from AddressBook if we know who it is
        let leader_id = raft.current_leader().await;
        println!("[_mesh.join] Current leader: {:?}", leader_id);
        let leader_data = match leader_id {
            Some(id) => raft
                .with_state_machine(|ab| ab.get_node(&id).cloned())
                .await,
            None => None,
        };
        println!("[_mesh.join] Returning NotLeader response");
        return Ok(MeshResponse::NotLeader { leader: leader_data });
    }

    // Serialize the Join command
    let command = AddressBookCommand::Join(req.clone());
    let bytes = RAFT_LOG_CODEC
        .encode(&command)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    // Submit to Raft
    println!("[_mesh.join] Submitting join command to Raft...");
    raft.submit_command(bytes)
        .await
        .map_err(crate::error::Error::from)?;

    println!("[_mesh.join] Join successful for node: {}", req.node_id);
    Ok(MeshResponse::Success)
}

/// Built-in handler for mesh leave requests
///
/// Allows nodes to leave the cluster gracefully. If this node is the leader,
/// it submits a Leave command to Raft. Otherwise, it returns information
/// about the current leader so the client can retry there.
#[handler(route = "_mesh.leave")]
async fn mesh_leave(
    req: LeaveRequest,
    raft: Data<RaftNode<AddressBook>>,
) -> Result<MeshResponse, crate::error::Error> {
    // Check if we're the leader
    if !raft.is_leader().await {
        // Get leader info from AddressBook if we know who it is
        let leader_id = raft.current_leader().await;
        let leader_data = match leader_id {
            Some(id) => raft
                .with_state_machine(|ab| ab.get_node(&id).cloned())
                .await,
            None => None,
        };
        return Ok(MeshResponse::NotLeader { leader: leader_data });
    }

    // Serialize the Leave command
    let command = AddressBookCommand::Leave(req.node_id);
    let bytes = RAFT_LOG_CODEC
        .encode(&command)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    // Submit to Raft
    raft.submit_command(bytes)
        .await
        .map_err(crate::error::Error::from)?;

    Ok(MeshResponse::Success)
}
