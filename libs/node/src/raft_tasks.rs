//! Raft consensus background tasks
//!
//! This module provides the scheduled tasks needed for Raft consensus:
//! - Election timeout: triggers elections when no heartbeat received
//! - Heartbeat: leader sends AppendEntries to all peers
//! - Apply committed: applies committed log entries to the state machine

use crate::mesh::{AddressBook, AddressBookCommand};
use crate::rpc::RpcClient;
use crate::scheduler::{Schedule, Scheduler, TaskContext};
use constellation_fabric::codec::{BincodeCodec, Codec};
use constellation_raft::RaftNode;
use std::time::Duration;

// Timing constants (per Raft paper recommendations)
const ELECTION_TIMEOUT_MIN: Duration = Duration::from_millis(150);
const ELECTION_TIMEOUT_MAX: Duration = Duration::from_millis(300);
const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(50);
const APPLY_INTERVAL: Duration = Duration::from_millis(10);

/// Schedule Raft background tasks (election timeout + heartbeat + apply)
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

    // Schedule apply committed entries task
    scheduler
        .schedule_named(
            "apply_committed",
            Schedule::every(APPLY_INTERVAL),
            apply_committed_task,
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

    let node_id = raft.node_id().await;

    // Don't start elections if we're already leader
    if raft.is_leader().await {
        return;
    }

    // Don't start elections if we can't lead
    if !raft.can_lead().await {
        return;
    }

    println!("[{}] Election timeout fired, starting election...", node_id);

    // Start election
    if let Err(e) = raft.start_election().await {
        eprintln!("election_timeout: Failed to start election: {}", e);
        return;
    }

    // Check if we already won (single-node case)
    if raft.is_leader().await {
        println!("[{}] Won election immediately (single-node majority)", node_id);
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

    // Get peers from AddressBook (source of truth for cluster membership)
    let self_id = raft.node_id().await;
    let peers: Vec<String> = raft
        .with_state_machine(|ab| {
            ab.all_nodes()
                .keys()
                .filter(|id| *id != &self_id)
                .cloned()
                .collect()
        })
        .await;

    println!("[{}] Requesting votes from {} peers: {:?}", node_id, peers.len(), peers);

    // Send vote requests to all peers
    for peer_id in peers {
        println!("[{}] Sending vote request to {}", node_id, peer_id);

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
                let resp: constellation_raft::RequestVoteResponse = resp;
                println!("[{}] Got vote response from {}: granted={}", node_id, peer_id, resp.vote_granted);
                // Handle vote response
                match raft.handle_request_vote_response(&peer_id, resp).await {
                    Ok(constellation_raft::ElectionResult::Won) => {
                        println!("[{}] Won election!", node_id);
                        return;
                    }
                    Ok(constellation_raft::ElectionResult::Lost(term)) => {
                        println!("[{}] Lost election (saw term {})", node_id, term);
                        return;
                    }
                    Ok(constellation_raft::ElectionResult::StillVoting) => {
                        println!("[{}] Still collecting votes...", node_id);
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
                println!("[{}] Vote request to {} failed: {}", node_id, peer_id, e);
            }
        }
    }

    println!("[{}] Election round complete, still candidate", node_id);
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

    // Get peers from AddressBook (source of truth for cluster membership)
    let self_id = raft.node_id().await;
    let peers: Vec<String> = raft
        .with_state_machine(|ab| {
            ab.all_nodes()
                .keys()
                .filter(|id| *id != &self_id)
                .cloned()
                .collect()
        })
        .await;

    // Send heartbeats to all peers
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

/// Apply committed entries to the state machine
///
/// This task runs frequently to apply any entries that have been
/// committed but not yet applied to the AddressBook.
async fn apply_committed_task(ctx: TaskContext) {
    // Extract dependencies
    let Some(raft) = ctx.extract::<RaftNode<AddressBook>>() else {
        eprintln!("apply_committed: Failed to extract RaftNode");
        return;
    };

    // Get the starting index for tracking
    let start_index = raft.last_applied().await + 1;

    // Get unapplied entries (raft returns raw LogEntry)
    let entries = match raft.get_unapplied_entries().await {
        Ok(e) => e,
        Err(e) => {
            eprintln!("apply_committed: Failed to get unapplied entries: {}", e);
            return;
        }
    };

    if entries.is_empty() {
        return;
    }

    // Apply each entry in order
    for (i, entry) in entries.into_iter().enumerate() {
        let index = start_index + i as u64;

        // Deserialize command from raw bytes (node crate handles codec)
        let command: AddressBookCommand = match BincodeCodec.decode(&entry.command) {
            Ok(cmd) => cmd,
            Err(e) => {
                eprintln!(
                    "apply_committed: Failed to deserialize command at index {}: {}",
                    index, e
                );
                // This shouldn't happen - it means corrupted log entry
                // Stop processing to avoid applying out of order
                break;
            }
        };

        // Apply to state machine
        match raft.apply_to_state_machine(command).await {
            Ok(_response) => {
                // Mark this entry as applied
                if let Err(e) = raft.mark_applied(index).await {
                    eprintln!(
                        "apply_committed: Failed to mark index {} as applied: {}",
                        index, e
                    );
                    break;
                }
            }
            Err(e) => {
                eprintln!(
                    "apply_committed: Failed to apply command at index {}: {}",
                    index, e
                );
                // Stop on first failure - entries must be applied in order
                break;
            }
        }
    }
}
