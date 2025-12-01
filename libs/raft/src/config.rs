//! Raft configuration
//!
//! Configuration values for the Raft consensus algorithm.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;

/// Raft algorithm configuration
#[derive(Debug, Clone, Serialize, Deserialize, SmartDefault, JsonSchema)]
#[serde(default)]
pub struct RaftConfig {
    /// Number of log entries after which to trigger snapshot creation (default: 1000)
    #[default = 1000]
    pub snapshot_threshold: u64,
}
