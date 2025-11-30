// Built-in handlers for framework-internal RPC

use crate::handler;
use crate::mesh::AddressBook;
use crate::scheduler::Scheduler;
use crate::Data;
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
