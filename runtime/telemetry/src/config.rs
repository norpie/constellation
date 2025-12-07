//! Configuration for the Telemetry Service

use serde::{Deserialize, Serialize};

/// Configuration for the telemetry service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryServiceConfig {
    /// Node ID for this telemetry service instance
    pub node_id: String,

    /// Address to listen on
    pub listen_addr: String,

    /// Interval between scrapes (in seconds)
    pub scrape_interval_secs: u64,

    /// Path to datapad storage
    pub storage_path: String,
}

impl Default for TelemetryServiceConfig {
    fn default() -> Self {
        Self {
            node_id: "telemetry-service".to_string(),
            listen_addr: "127.0.0.1:9090".to_string(),
            scrape_interval_secs: 15,
            storage_path: "./telemetry-data".to_string(),
        }
    }
}
