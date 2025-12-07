//! Configuration for the Telemetry Service

use serde::{Deserialize, Serialize};

/// Configuration for the telemetry service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryServiceConfig {
    /// Node ID for this telemetry service instance
    pub node_id: String,

    /// Address to listen on
    pub listen_addr: String,

    /// Interval between remote node scrapes (in seconds)
    pub scrape_interval_secs: u64,

    /// Interval between local collector drains (in seconds)
    /// This drains the telemetry service's own collector directly into storage.
    pub self_drain_interval_secs: u64,

    /// Path to datapad storage
    pub storage_path: String,
}

impl Default for TelemetryServiceConfig {
    fn default() -> Self {
        Self {
            node_id: "telemetry-service".to_string(),
            listen_addr: "127.0.0.1:9090".to_string(),
            scrape_interval_secs: 15,
            self_drain_interval_secs: 5,
            storage_path: "./telemetry-data".to_string(),
        }
    }
}
