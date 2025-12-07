//! Constellation Telemetry Service
//!
//! Central service for collecting, storing, and querying telemetry data
//! (logs, metrics, traces) from all nodes in the mesh.

use constellation_datapad::{Datapad, DatapadConfig, StorageMode};
use constellation_fabric::transport::TcpTransportListener;
use constellation_node::{Binding, Node};

mod config;
mod error;
mod handlers;
mod scraper;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::TelemetryServiceConfig::default();

    println!("Starting Telemetry Service on {}", config.listen_addr);

    // Initialize storage
    let datapad_config = DatapadConfig {
        storage: StorageMode::Path(config.storage_path.clone().into()),
        ..Default::default()
    };
    let datapad = Datapad::open(&datapad_config)?;

    println!("Datapad storage initialized at {}", config.storage_path);

    // Bind listener
    let listener = TcpTransportListener::bind(config.listen_addr.parse()?)
        .await?;

    let binding = Binding::new(listener, "tcp")
        .advertise("default", &config.listen_addr);

    // Build node
    let node = Node::builder()
        .service_name("TelemetryService")
        .id(&config.node_id)
        .binding(binding)
        .data(datapad)
        // TODO: Add scraper config
        // TODO: Schedule scraper task
        .build()?;

    println!("Telemetry Service built, starting...");

    let running = node.start().await?;

    println!("Telemetry Service running. Press Ctrl+C to stop.");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;

    println!("Shutting down...");
    running.shutdown();

    Ok(())
}
