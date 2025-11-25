// Built-in handlers for framework-internal RPC

use crate::handler;
use crate::mesh::AddressBook;
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
#[handler(route = "_raft.append_entries")]
async fn append_entries(
    req: AppendEntriesRequest,
    raft: Data<RaftNode<AddressBook>>,
) -> Result<AppendEntriesResponse, crate::error::Error> {
    raft.handle_append_entries(req)
        .await
        .map_err(crate::error::Error::from)
}
