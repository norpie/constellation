//! Minimal 3-node cluster example
//!
//! Node-1 starts first and forms a new cluster (no bootstrap peers).
//! Node-2 and Node-3 join by connecting to Node-1.

use constellation_fabric::transport::TcpTransportListener;
use constellation_node::mesh::AddressGroup;
use constellation_node::Node;
use std::time::Duration;

const NODE1_ADDR: &str = "127.0.0.1:9001";
const NODE2_ADDR: &str = "127.0.0.1:9002";
const NODE3_ADDR: &str = "127.0.0.1:9003";

#[tokio::main]
async fn main() {
    println!("Starting 3-node cluster...\n");

    // Node-1 starts first with NO bootstrap peers (forms new cluster)
    let h1 = tokio::spawn(run_node("node-1", NODE1_ADDR, vec![]));

    // Give node-1 time to start and become leader
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Node-2 and Node-3 join via node-1
    let h2 = tokio::spawn(run_node("node-2", NODE2_ADDR, vec![NODE1_ADDR]));
    let h3 = tokio::spawn(run_node("node-3", NODE3_ADDR, vec![NODE1_ADDR]));

    // Let them run for a bit
    tokio::time::sleep(Duration::from_secs(10)).await;

    println!("\n--- 10 seconds elapsed, shutting down ---");

    h1.abort();
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
