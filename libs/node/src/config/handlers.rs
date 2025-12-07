//! Built-in handlers for config management

use crate::config::{get_json_path, set_json_path, Config};
use crate::handler;
use crate::Data;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Request for getting config values
#[derive(Debug, Deserialize)]
pub struct ConfigGetRequest {
    /// Optional dot-separated path. If empty, returns full config.
    #[serde(default)]
    pub path: Option<String>,
}

/// Request for setting a config value
#[derive(Debug, Deserialize)]
pub struct ConfigSetRequest {
    /// Dot-separated path to the config field
    pub path: String,
    /// New value to set
    pub value: serde_json::Value,
}

/// Response for config operations
#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Empty request for handlers that don't need input
#[derive(Debug, Deserialize)]
pub struct EmptyRequest {}

/// Get config value(s)
///
/// If path is provided, returns the value at that path.
/// Otherwise, returns the full config.
#[handler(route = "_config.get")]
async fn config_get(
    req: ConfigGetRequest,
    config: Data<RwLock<Config>>,
) -> Result<serde_json::Value, crate::error::Error> {
    let cfg = config.read().await;
    let json = serde_json::to_value(&*cfg)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    match req.path {
        Some(path) if !path.is_empty() => {
            let value = get_json_path(&json, &path)
                .map_err(|e| crate::error::Error::Custom(e.to_string()))?;
            Ok(value.clone())
        }
        _ => Ok(json),
    }
}

/// Set a config value
///
/// The value is validated by deserializing the entire config after modification.
/// If validation fails, the config is not changed.
#[handler(route = "_config.set")]
async fn config_set(
    req: ConfigSetRequest,
    config: Data<RwLock<Config>>,
) -> Result<ConfigResponse, crate::error::Error> {
    let mut cfg = config.write().await;

    // Serialize current config to JSON
    let mut json = serde_json::to_value(&*cfg)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    // Modify at path
    set_json_path(&mut json, &req.path, req.value)
        .map_err(|e| crate::error::Error::Custom(e.to_string()))?;

    // Deserialize back - this validates the change
    *cfg = serde_json::from_value(json)
        .map_err(|e| crate::error::Error::Custom(format!("Invalid config: {}", e)))?;

    Ok(ConfigResponse {
        success: true,
        error: None,
    })
}

/// Get JSON Schema for the config
///
/// If path is provided, returns the schema for that field.
/// Otherwise, returns the full schema.
#[handler(route = "_config.schema")]
async fn config_schema(
    req: ConfigGetRequest,
) -> Result<serde_json::Value, crate::error::Error> {
    let schema = schemars::schema_for!(Config);
    let json = serde_json::to_value(&schema)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    match req.path {
        Some(path) if !path.is_empty() => {
            // Convert path to schema path: "rpc.timeout" -> "properties.rpc.properties.timeout"
            let schema_path = path
                .split('.')
                .map(|s| format!("properties.{}", s))
                .collect::<Vec<_>>()
                .join(".");

            let value = get_json_path(&json, &schema_path)
                .map_err(|e| crate::error::Error::Custom(e.to_string()))?;
            Ok(value.clone())
        }
        _ => Ok(json),
    }
}

/// Reset config to defaults
#[handler(route = "_config.reset")]
async fn config_reset(
    _req: EmptyRequest,
    config: Data<RwLock<Config>>,
) -> Result<ConfigResponse, crate::error::Error> {
    let mut cfg = config.write().await;
    *cfg = Config::default();

    Ok(ConfigResponse {
        success: true,
        error: None,
    })
}
