//! Minimal 3-node cluster example with leader failover
//!
//! Node-1 starts first and forms a new cluster (no bootstrap peers).
//! Node-2 and Node-3 join by connecting to Node-1.
//! Then node-1 is killed to test leader election.

use constellation_fabric::transport::TcpTransportListener;
use constellation_node::mesh::AddressGroup;
use constellation_node::Node;
use std::time::Duration;

const NODE1_ADDR: &str = "127.0.0.1:9001";
const NODE2_ADDR: &str = "127.0.0.1:9002";
const NODE3_ADDR: &str = "127.0.0.1:9003";

#[tokio::main]
async fn main() {
    println!("=== Starting 3-node cluster ===\n");

    // Node-1 starts first with NO bootstrap peers (forms new cluster)
    let h1 = tokio::spawn(run_node("node-1", NODE1_ADDR, vec![]));

    // Give node-1 time to start and become leader
    println!("Waiting for node-1 to become leader...");
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Node-2 and Node-3 join via node-1
    let h2 = tokio::spawn(run_node("node-2", NODE2_ADDR, vec![NODE1_ADDR]));
    let h3 = tokio::spawn(run_node("node-3", NODE3_ADDR, vec![NODE1_ADDR]));

    // Let cluster stabilize
    println!("\nWaiting 2 seconds for cluster to stabilize...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Kill the leader
    println!("\n=== KILLING NODE-1 (the leader) ===\n");
    h1.abort();

    // Wait for election timeout (150-300ms) + some buffer
    println!("Waiting for new leader election (election timeout is 150-300ms)...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Let it run a bit more to see heartbeats
    println!("\nCluster running with 2 nodes for 5 more seconds...");
    tokio::time::sleep(Duration::from_secs(5)).await;

    println!("\n=== Shutting down ===");
    h2.abort();
    h3.abort();
}

async fn run_node(node_id: &str, listen_addr: &str, bootstrap_peers: Vec<&str>) {
    println!("[{}] Starting on {}", node_id, listen_addr);

    // Bind listener
    let listener = TcpTransportListener::bind(listen_addr.parse().unwrap())
        .await
        .expect("Failed to bind");

    // Build node
    let mut builder = Node::builder()
        .service_name("ClusterTest")
        .id(node_id)
        .listen(listener, "default", "tcp", listen_addr);

    // Add bootstrap peers (if any)
    for peer_addr in bootstrap_peers {
        let peer_id = match peer_addr {
            NODE1_ADDR => "node-1",
            NODE2_ADDR => "node-2",
            NODE3_ADDR => "node-3",
            _ => unreachable!(),
        };
        builder = builder.with_peer(
            peer_id,
            AddressGroup::single("default", "tcp", peer_addr),
        );
    }

    let node = builder.build().expect("Failed to build node");

    println!("[{}] Node built, starting...", node_id);

    if let Err(e) = node.start().await {
        println!("[{}] Node error: {}", node_id, e);
    }
}
