//! Constellation Telemetry Service
//!
//! Central service for collecting, storing, and querying telemetry data
//! (logs, metrics, traces) from all nodes in the mesh.

use std::time::Duration;

use constellation_datapad::{Datapad, DatapadConfig, StorageMode};
use constellation_fabric::transport::TcpTransportListener;
use constellation_node::{Binding, Node, Schedule};

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
    let listener = TcpTransportListener::bind(config.listen_addr.parse()?).await?;

    let binding = Binding::new(listener, "tcp").advertise("default", &config.listen_addr);

    // Schedule intervals
    let scrape_interval = Duration::from_secs(config.scrape_interval_secs);
    let drain_interval = Duration::from_secs(config.self_drain_interval_secs);

    // Build node
    let node = Node::builder()
        .service_name("TelemetryService")
        .id(&config.node_id)
        .binding(binding)
        .data(datapad)
        // Schedule remote scraper task
        .schedule_named(
            "remote-scraper",
            Schedule::every(scrape_interval),
            scraper::scrape_remote_nodes,
        )
        // Schedule local drain task
        .schedule_named(
            "self-drain",
            Schedule::every(drain_interval),
            scraper::drain_local_collector,
        )
        .build()?;

    println!("Telemetry Service built, starting...");
    println!(
        "  Remote scrape interval: {}s",
        config.scrape_interval_secs
    );
    println!(
        "  Self drain interval: {}s",
        config.self_drain_interval_secs
    );

    let running = node.start().await?;

    println!("Telemetry Service running. Press Ctrl+C to stop.");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;

    println!("Shutting down...");
    running.shutdown();

    Ok(())
}
