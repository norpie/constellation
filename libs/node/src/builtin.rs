// Built-in handlers for framework-internal RPC

use crate::handler;
use crate::mesh::{AddressBook, AddressBookCommand, LeaveRequest, MeshResponse, TransponderData};
use crate::scheduler::Scheduler;
use crate::Data;
use constellation_fabric::codec::BincodeCodec;
use constellation_raft::{
    AppendEntriesRequest, AppendEntriesResponse, RaftNode, RequestVoteRequest, RequestVoteResponse,
};

/// Built-in handler for Raft RequestVote RPC
#[handler(route = "_raft.request_vote")]
async fn request_vote(
    req: RequestVoteRequest,
    raft: Data<RaftNode<AddressBook>>,
) -> Result<RequestVoteResponse, crate::error::Error> {
    raft.handle_request_vote(req)
        .await
        .map_err(crate::error::Error::from)
}

/// Built-in handler for Raft AppendEntries RPC
///
/// Handles both heartbeats and log replication from the leader.
/// On success, resets the election timeout to prevent unnecessary elections.
#[handler(route = "_raft.append_entries")]
async fn append_entries(
    req: AppendEntriesRequest,
    raft: Data<RaftNode<AddressBook>>,
    scheduler: Data<Scheduler>,
) -> Result<AppendEntriesResponse, crate::error::Error> {
    let resp = raft
        .handle_append_entries(req)
        .await
        .map_err(crate::error::Error::from)?;

    // Reset election timeout on successful heartbeat from leader
    // This prevents followers from starting unnecessary elections
    if resp.success {
        if let Some(handle) = scheduler.handle_by_name("election_timeout").await {
            handle.reset_now();
        }
    }

    Ok(resp)
}

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

    // Serialize the Join command
    let command = AddressBookCommand::Join(req);
    let bytes = BincodeCodec
        .encode(&command)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    // Submit to Raft
    raft.submit_command(bytes)
        .await
        .map_err(crate::error::Error::from)?;

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
    let bytes = BincodeCodec
        .encode(&command)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    // Submit to Raft
    raft.submit_command(bytes)
        .await
        .map_err(crate::error::Error::from)?;

    Ok(MeshResponse::Success)
}
