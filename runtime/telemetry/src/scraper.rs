//! Telemetry scraper
//!
//! Periodically scrapes telemetry data from nodes in the mesh
//! and drains the local collector.

use constellation_datapad::Datapad;
use constellation_node::mesh::AddressBook;
use constellation_node::scheduler::{NodeIdentity, TaskContext};
use constellation_node::{Data, RpcClient};
use constellation_raft::RaftNode;
use constellation_telemetry::{BufferCollector, Collector, TelemetryEntry};
use serde::{Deserialize, Serialize};

// ----------------------------------------------------------------------------
// Types matching the node's _telemetry.scrape handler
// ----------------------------------------------------------------------------

/// Response containing collected telemetry entries (matches node's ScrapeResponse)
#[derive(Debug, Serialize, Deserialize)]
struct ScrapeResponse {
    entries: Vec<TelemetryEntry>,
}

// ----------------------------------------------------------------------------
// Scraper Tasks
// ----------------------------------------------------------------------------

/// Scrape telemetry from all remote nodes in the mesh.
///
/// This task:
/// 1. Gets the list of nodes from the AddressBook
/// 2. Skips self (telemetry service)
/// 3. Calls `_telemetry.scrape` on each node
/// 4. Ingests the results into Datapad
///
/// Errors are logged but don't stop the scrape - we continue to the next node.
/// No telemetry is emitted from this task to avoid recursive loops.
pub async fn scrape_remote_nodes(ctx: TaskContext) {
    // Extract dependencies
    let rpc: Data<RpcClient> = match ctx.extract() {
        Some(r) => r,
        None => {
            eprintln!("[scraper] RpcClient not available");
            return;
        }
    };
    let raft: Data<RaftNode<AddressBook>> = match ctx.extract() {
        Some(r) => r,
        None => {
            eprintln!("[scraper] RaftNode not available");
            return;
        }
    };
    let datapad: Data<Datapad> = match ctx.extract() {
        Some(d) => d,
        None => {
            eprintln!("[scraper] Datapad not available");
            return;
        }
    };
    let self_id: Data<NodeIdentity> = match ctx.extract() {
        Some(i) => i,
        None => {
            eprintln!("[scraper] NodeIdentity not available");
            return;
        }
    };

    // Get all nodes from the address book
    let nodes: Vec<String> = raft
        .with_state_machine(|ab| ab.all_nodes().keys().cloned().collect())
        .await;

    let self_node_id = &self_id.0;

    for node_id in nodes {
        // Skip self
        if &node_id == self_node_id {
            continue;
        }

        // Call _telemetry.scrape on the node
        let call_result = rpc.call_peer::<(), ScrapeResponse>(
            &node_id,
            "_telemetry.scrape",
            &(),
        );

        let builder = match call_result {
            Ok(b) => b.no_retry(),
            Err(e) => {
                eprintln!("[scraper] Failed to build RPC call for {}: {}", node_id, e);
                continue;
            }
        };

        match builder.await {
            Ok(response) => {
                if !response.entries.is_empty() {
                    let count = response.entries.len();
                    if let Err(e) = datapad.insert_batch(&response.entries) {
                        eprintln!(
                            "[scraper] Failed to ingest {} entries from {}: {}",
                            count, node_id, e
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!("[scraper] Scrape failed for {}: {}", node_id, e);
            }
        }
    }
}

/// Drain the local collector directly into storage.
///
/// This task bypasses RPC and directly drains the telemetry service's
/// own BufferCollector into Datapad. This avoids the telemetry service
/// calling `_telemetry.scrape` on itself.
///
/// No telemetry is emitted from this task to avoid recursive loops.
pub async fn drain_local_collector(ctx: TaskContext) {
    // Extract dependencies
    let collector: Data<BufferCollector> = match ctx.extract() {
        Some(c) => c,
        None => {
            eprintln!("[scraper] BufferCollector not available");
            return;
        }
    };
    let datapad: Data<Datapad> = match ctx.extract() {
        Some(d) => d,
        None => {
            eprintln!("[scraper] Datapad not available");
            return;
        }
    };

    let entries = collector.drain();

    if !entries.is_empty() {
        let count = entries.len();
        if let Err(e) = datapad.insert_batch(&entries) {
            eprintln!("[scraper] Failed to ingest {} local entries: {}", count, e);
        }
    }
}
