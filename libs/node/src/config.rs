//! Configuration system for the node framework
//!
//! Provides a typed configuration struct that can be accessed directly
//! for performance, while also supporting dynamic introspection via
//! serde for management routes.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;

/// Framework configuration
///
/// This struct holds all configurable settings for the node framework.
/// Access it via `Data<RwLock<Config>>` in handlers and tasks.
#[derive(Debug, Clone, Serialize, Deserialize, SmartDefault, JsonSchema)]
#[serde(default)]
pub struct Config {
    /// Placeholder - will be replaced with real config sections
    #[default = 42]
    pub placeholder: u64,
}
