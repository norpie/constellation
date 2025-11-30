//! Raft consensus background tasks
//!
//! This module provides the scheduled tasks needed for Raft consensus:
//! - Election timeout: triggers elections when no heartbeat received
//! - Heartbeat: leader sends AppendEntries to all peers

use crate::mesh::AddressBook;
use crate::rpc::RpcClient;
use crate::scheduler::{Schedule, Scheduler, TaskContext};
use constellation_raft::RaftNode;
use std::time::Duration;

// Timing constants (per Raft paper recommendations)
const ELECTION_TIMEOUT_MIN: Duration = Duration::from_millis(150);
const ELECTION_TIMEOUT_MAX: Duration = Duration::from_millis(300);
const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(50);

/// Schedule Raft background tasks (election timeout + heartbeat)
///
/// This should be called during node startup after the scheduler is running.
pub async fn schedule_raft_tasks(scheduler: &Scheduler) -> crate::Result<()> {
    // Schedule election timeout task
    scheduler
        .schedule_named(
            "election_timeout",
            Schedule::random_interval(ELECTION_TIMEOUT_MIN, ELECTION_TIMEOUT_MAX),
            election_timeout_task,
        )
        .await?;

    // Schedule heartbeat task
    scheduler
        .schedule_named(
            "leader_heartbeat",
            Schedule::every(HEARTBEAT_INTERVAL),
            heartbeat_task,
        )
        .await?;

    Ok(())
}

/// Election timeout task - starts election when timeout fires
///
/// This task fires when we haven't received a heartbeat from the leader.
/// It triggers an election by becoming a candidate and requesting votes.
async fn election_timeout_task(ctx: TaskContext) {
    // Extract dependencies
    let Some(raft) = ctx.extract::<RaftNode<AddressBook>>() else {
        eprintln!("election_timeout: Failed to extract RaftNode");
        return;
    };

    let Some(rpc) = ctx.extract::<RpcClient>() else {
        eprintln!("election_timeout: Failed to extract RpcClient");
        return;
    };

    // Don't start elections if we're already leader
    if raft.is_leader().await {
        return;
    }

    // Don't start elections if we can't lead
    if !raft.can_lead().await {
        return;
    }

    // Start election
    if let Err(e) = raft.start_election().await {
        eprintln!("election_timeout: Failed to start election: {}", e);
        return;
    }

    // Prepare vote request
    let request = match raft.prepare_request_vote().await {
        Ok(req) => req,
        Err(e) => {
            eprintln!("election_timeout: Failed to prepare vote request: {}", e);
            return;
        }
    };

    // Send vote requests to all peers
    let peers = raft.peers().await;
    for peer_id in peers {
        // Send vote request to peer
        let response = match rpc.call_peer(&peer_id, "_raft.request_vote.v1", &request) {
            Ok(builder) => builder.await,
            Err(e) => {
                eprintln!(
                    "election_timeout: Failed to serialize vote request for {}: {}",
                    peer_id, e
                );
                continue;
            }
        };

        match response {
            Ok(resp) => {
                // Handle vote response
                match raft.handle_request_vote_response(&peer_id, resp).await {
                    Ok(constellation_raft::ElectionResult::Won) => {
                        // We won! The heartbeat task will now start sending
                        return;
                    }
                    Ok(constellation_raft::ElectionResult::Lost(_)) => {
                        // We lost (saw higher term), stop requesting votes
                        return;
                    }
                    Ok(constellation_raft::ElectionResult::StillVoting) => {
                        // Continue collecting votes
                    }
                    Err(e) => {
                        eprintln!(
                            "election_timeout: Failed to handle vote response from {}: {}",
                            peer_id, e
                        );
                    }
                }
            }
            Err(e) => {
                // RPC failed - peer might be down, continue with other peers
                eprintln!(
                    "election_timeout: Failed to send vote request to {}: {}",
                    peer_id, e
                );
            }
        }
    }
}

/// Leader heartbeat task - sends AppendEntries to all peers
///
/// This task runs at a fixed interval and sends heartbeats (empty AppendEntries)
/// to all peers when this node is the leader.
async fn heartbeat_task(ctx: TaskContext) {
    // Extract dependencies
    let Some(raft) = ctx.extract::<RaftNode<AddressBook>>() else {
        eprintln!("heartbeat: Failed to extract RaftNode");
        return;
    };

    let Some(rpc) = ctx.extract::<RpcClient>() else {
        eprintln!("heartbeat: Failed to extract RpcClient");
        return;
    };

    // Only send heartbeats if we're the leader
    if !raft.is_leader().await {
        return;
    }

    // Send heartbeats to all peers
    let peers = raft.peers().await;
    for peer_id in peers {
        // Prepare AppendEntries for this peer
        let request = match raft.prepare_append_entries(&peer_id).await {
            Ok(req) => req,
            Err(e) => {
                eprintln!(
                    "heartbeat: Failed to prepare append_entries for {}: {}",
                    peer_id, e
                );
                continue;
            }
        };

        // Send AppendEntries to peer
        let response = match rpc.call_peer(&peer_id, "_raft.append_entries.v1", &request) {
            Ok(builder) => builder.await,
            Err(e) => {
                eprintln!(
                    "heartbeat: Failed to serialize append_entries for {}: {}",
                    peer_id, e
                );
                continue;
            }
        };

        match response {
            Ok(resp) => {
                // Handle response
                if let Err(e) = raft.handle_append_entries_response(&peer_id, resp).await {
                    eprintln!(
                        "heartbeat: Failed to handle append_entries response from {}: {}",
                        peer_id, e
                    );
                }
            }
            Err(e) => {
                // RPC failed - peer might be down
                eprintln!(
                    "heartbeat: Failed to send append_entries to {}: {}",
                    peer_id, e
                );
            }
        }
    }
}
