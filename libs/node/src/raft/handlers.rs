//! Built-in handlers for Raft RPC

use crate::handler;
use crate::mesh::AddressBook;
use crate::scheduler::Scheduler;
use crate::Data;
use constellation_raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    RaftNode, RequestVoteRequest, RequestVoteResponse,
};

/// Built-in handler for Raft RequestVote RPC
#[handler(route = "_raft.request_vote")]
async fn request_vote(
    req: RequestVoteRequest,
    raft: Data<RaftNode<AddressBook>>,
    scheduler: Data<Scheduler>,
) -> Result<RequestVoteResponse, crate::error::Error> {
    let resp = raft
        .handle_request_vote(req)
        .await
        .map_err(crate::error::Error::from)?;

    // Reset election timeout when granting a vote (per Raft spec)
    // This prevents the voter from immediately starting its own election
    if resp.vote_granted {
        if let Some(handle) = scheduler.handle_by_name("election_timeout").await {
            handle.reset_now();
        }
    }

    Ok(resp)
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

/// Built-in handler for Raft InstallSnapshot RPC
///
/// Handles snapshot installation from the leader when a follower is too far
/// behind to catch up via normal log replication.
#[handler(route = "_raft.install_snapshot")]
async fn install_snapshot(
    req: InstallSnapshotRequest,
    raft: Data<RaftNode<AddressBook>>,
    scheduler: Data<Scheduler>,
) -> Result<InstallSnapshotResponse, crate::error::Error> {
    let resp = raft
        .handle_install_snapshot(req)
        .await
        .map_err(crate::error::Error::from)?;

    // Reset election timeout - valid leader contact
    if let Some(handle) = scheduler.handle_by_name("election_timeout").await {
        handle.reset_now();
    }

    Ok(resp)
}
