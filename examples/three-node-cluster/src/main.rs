//! Minimal 3-node cluster example with leader failover
//!
//! Node-1 starts first and forms a new cluster (no bootstrap peers).
//! Node-2 and Node-3 join by connecting to Node-1.
//! Then node-1 is killed to test leader election.

use constellation_fabric::transport::TcpTransportListener;
use constellation_node::mesh::AdvertisedAddress;
use constellation_node::Node;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const NODE1_ADDR: &str = "127.0.0.1:9001";
const NODE2_ADDR: &str = "127.0.0.1:9002";
const NODE3_ADDR: &str = "127.0.0.1:9003";

#[tokio::main]
async fn main() {
    println!("=== Starting 3-node cluster ===\n");

    // Shared storage for running nodes (so we can call shutdown)
    let nodes: Arc<Mutex<Vec<Arc<constellation_node::Node>>>> = Arc::new(Mutex::new(Vec::new()));

    // Node-1 starts first with NO bootstrap peers (forms new cluster)
    let nodes_clone = Arc::clone(&nodes);
    let h1 = tokio::spawn(async move {
        if let Some(node) = run_node("node-1", NODE1_ADDR, vec![]).await {
            nodes_clone.lock().await.push(node);
        }
    });

    // Give node-1 time to start and become leader
    println!("Waiting for node-1 to become leader...");
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Node-2 and Node-3 join via node-1
    let nodes_clone = Arc::clone(&nodes);
    let h2 = tokio::spawn(async move {
        if let Some(node) = run_node("node-2", NODE2_ADDR, vec![NODE1_ADDR]).await {
            nodes_clone.lock().await.push(node);
        }
    });

    let nodes_clone = Arc::clone(&nodes);
    let h3 = tokio::spawn(async move {
        if let Some(node) = run_node("node-3", NODE3_ADDR, vec![NODE1_ADDR]).await {
            nodes_clone.lock().await.push(node);
        }
    });

    // Let cluster stabilize
    println!("\nWaiting 2 seconds for cluster to stabilize...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Shutdown node-1 (the leader)
    println!("\n=== SHUTTING DOWN NODE-1 (the leader) ===\n");
    {
        let nodes = nodes.lock().await;
        if let Some(node1) = nodes.first() {
            node1.shutdown();
        }
    }

    // Wait for election timeout (150-300ms) + some buffer
    println!("Waiting for new leader election (election timeout is 150-300ms)...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Let it run a bit more to see heartbeats
    println!("\nCluster running with 2 nodes for 5 more seconds...");
    tokio::time::sleep(Duration::from_secs(5)).await;

    println!("\n=== Shutting down all nodes ===");
    {
        let nodes = nodes.lock().await;
        for node in nodes.iter() {
            node.shutdown();
        }
    }

    // Give tasks time to stop
    tokio::time::sleep(Duration::from_millis(200)).await;
}

async fn run_node(node_id: &str, listen_addr: &str, bootstrap_peers: Vec<&str>) -> Option<Arc<constellation_node::Node>> {
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
            AdvertisedAddress::new("default", "tcp", peer_addr),
        );
    }

    let node = builder.build().expect("Failed to build node");

    println!("[{}] Node built, starting...", node_id);

    match node.start().await {
        Ok(running_node) => {
            println!("[{}] Node started successfully", node_id);
            Some(running_node)
        }
        Err(e) => {
            println!("[{}] Node error: {}", node_id, e);
            None
        }
    }
}
