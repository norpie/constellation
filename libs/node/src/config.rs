//! Configuration system for the node framework
//!
//! Provides a typed configuration struct that can be accessed directly
//! for performance, while also supporting dynamic introspection via
//! serde for management routes.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use smart_default::SmartDefault;

/// Framework configuration
///
/// This struct holds all configurable settings for the node framework.
/// Access it via `Data<RwLock<Config>>` in handlers and tasks.
#[derive(Debug, Clone, Serialize, Deserialize, SmartDefault, JsonSchema)]
#[serde(default)]
pub struct Config {
    /// Raft consensus timing configuration (node-level: election, heartbeat)
    pub raft: RaftConfig,
    /// Raft algorithm configuration (crate-level: snapshot threshold)
    #[default(RaftCrateConfig::default())]
    pub raft_crate: RaftCrateConfig,
    /// RPC client retry and timeout configuration
    pub rpc: RpcConfig,
    /// Scheduler configuration
    pub scheduler: SchedulerConfig,
}

/// Re-export raft crate's config for convenience
pub use constellation_raft::RaftConfig as RaftCrateConfig;

/// Raft consensus timing configuration
#[derive(Debug, Clone, Serialize, Deserialize, SmartDefault, JsonSchema)]
#[serde(default)]
pub struct RaftConfig {
    /// Minimum election timeout in milliseconds (default: 150)
    #[default = 150]
    pub election_timeout_min_ms: u64,
    /// Maximum election timeout in milliseconds (default: 300)
    #[default = 300]
    pub election_timeout_max_ms: u64,
    /// Leader heartbeat interval in milliseconds (default: 50)
    #[default = 50]
    pub heartbeat_interval_ms: u64,
    /// Interval to check for committed entries to apply in milliseconds (default: 10)
    #[default = 10]
    pub apply_interval_ms: u64,
}

/// RPC client retry and timeout configuration
#[derive(Debug, Clone, Serialize, Deserialize, SmartDefault, JsonSchema)]
#[serde(default)]
pub struct RpcConfig {
    /// Default maximum retry attempts (default: 3)
    #[default = 3]
    pub max_attempts: u32,
    /// Default timeout per attempt in milliseconds (default: 5000)
    #[default = 5000]
    pub timeout_per_attempt_ms: u64,
    /// Initial backoff delay for exponential retry in milliseconds (default: 100)
    #[default = 100]
    pub initial_backoff_ms: u64,
    /// Maximum backoff delay cap in milliseconds (default: 5000)
    #[default = 5000]
    pub max_backoff_ms: u64,
}

/// Scheduler configuration
#[derive(Debug, Clone, Serialize, Deserialize, SmartDefault, JsonSchema)]
#[serde(default)]
pub struct SchedulerConfig {
    /// Command channel buffer size (default: 256)
    #[default = 256]
    pub channel_buffer_size: usize,
    /// Sleep duration when no tasks are scheduled, in seconds (default: 3600)
    #[default = 3600]
    pub idle_sleep_secs: u64,
}

/// Error type for path operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// The path was empty
    EmptyPath,
    /// A segment in the path was not found
    NotFound(String),
    /// Tried to traverse into a non-object value
    NotAnObject(String),
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathError::EmptyPath => write!(f, "path cannot be empty"),
            PathError::NotFound(path) => write!(f, "path not found: {}", path),
            PathError::NotAnObject(path) => write!(f, "not an object at: {}", path),
        }
    }
}

impl std::error::Error for PathError {}

/// Get a value from a JSON object by dot-separated path
///
/// # Example
/// ```
/// use serde_json::json;
/// use constellation_node::config::get_json_path;
///
/// let data = json!({"rpc": {"timeout_ms": 5000}});
/// let value = get_json_path(&data, "rpc.timeout_ms").unwrap();
/// assert_eq!(value, &json!(5000));
/// ```
pub fn get_json_path<'a>(root: &'a Value, path: &str) -> Result<&'a Value, PathError> {
    if path.is_empty() {
        return Err(PathError::EmptyPath);
    }

    let parts: Vec<&str> = path.split('.').collect();
    let mut current = root;

    for (i, part) in parts.iter().enumerate() {
        match current.get(part) {
            Some(v) => current = v,
            None => {
                let traversed = parts[..=i].join(".");
                return Err(PathError::NotFound(traversed));
            }
        }
    }

    Ok(current)
}

/// Set a value in a JSON object by dot-separated path
///
/// Creates intermediate objects if they don't exist.
///
/// # Example
/// ```
/// use serde_json::json;
/// use constellation_node::config::set_json_path;
///
/// let mut data = json!({"rpc": {"timeout_ms": 5000}});
/// set_json_path(&mut data, "rpc.timeout_ms", json!(10000)).unwrap();
/// assert_eq!(data["rpc"]["timeout_ms"], 10000);
/// ```
pub fn set_json_path(root: &mut Value, path: &str, value: Value) -> Result<(), PathError> {
    if path.is_empty() {
        return Err(PathError::EmptyPath);
    }

    let parts: Vec<&str> = path.split('.').collect();
    let mut current = root;

    // Navigate to parent of target
    for (i, part) in parts[..parts.len() - 1].iter().enumerate() {
        if !current.is_object() {
            let traversed = parts[..i].join(".");
            return Err(PathError::NotAnObject(traversed));
        }

        // Create intermediate object if it doesn't exist
        if current.get(part).is_none() {
            current[part] = Value::Object(serde_json::Map::new());
        }

        current = current.get_mut(part).unwrap();
    }

    // Set the final value
    let last = parts.last().unwrap();
    if !current.is_object() {
        let traversed = parts[..parts.len() - 1].join(".");
        return Err(PathError::NotAnObject(traversed));
    }

    current[last] = value;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_get_json_path_simple() {
        let data = json!({"foo": 42});
        assert_eq!(get_json_path(&data, "foo").unwrap(), &json!(42));
    }

    #[test]
    fn test_get_json_path_nested() {
        let data = json!({"rpc": {"timeout_ms": 5000, "retries": 3}});
        assert_eq!(get_json_path(&data, "rpc.timeout_ms").unwrap(), &json!(5000));
        assert_eq!(get_json_path(&data, "rpc.retries").unwrap(), &json!(3));
    }

    #[test]
    fn test_get_json_path_deeply_nested() {
        let data = json!({"a": {"b": {"c": {"d": "deep"}}}});
        assert_eq!(get_json_path(&data, "a.b.c.d").unwrap(), &json!("deep"));
    }

    #[test]
    fn test_get_json_path_not_found() {
        let data = json!({"foo": 42});
        assert_eq!(
            get_json_path(&data, "bar"),
            Err(PathError::NotFound("bar".to_string()))
        );
    }

    #[test]
    fn test_get_json_path_partial_not_found() {
        let data = json!({"rpc": {"timeout_ms": 5000}});
        assert_eq!(
            get_json_path(&data, "rpc.missing"),
            Err(PathError::NotFound("rpc.missing".to_string()))
        );
    }

    #[test]
    fn test_get_json_path_empty() {
        let data = json!({"foo": 42});
        assert_eq!(get_json_path(&data, ""), Err(PathError::EmptyPath));
    }

    #[test]
    fn test_set_json_path_simple() {
        let mut data = json!({"foo": 42});
        set_json_path(&mut data, "foo", json!(100)).unwrap();
        assert_eq!(data["foo"], 100);
    }

    #[test]
    fn test_set_json_path_nested() {
        let mut data = json!({"rpc": {"timeout_ms": 5000}});
        set_json_path(&mut data, "rpc.timeout_ms", json!(10000)).unwrap();
        assert_eq!(data["rpc"]["timeout_ms"], 10000);
    }

    #[test]
    fn test_set_json_path_new_field() {
        let mut data = json!({"rpc": {"timeout_ms": 5000}});
        set_json_path(&mut data, "rpc.retries", json!(5)).unwrap();
        assert_eq!(data["rpc"]["retries"], 5);
        assert_eq!(data["rpc"]["timeout_ms"], 5000); // unchanged
    }

    #[test]
    fn test_set_json_path_create_intermediate() {
        let mut data = json!({});
        set_json_path(&mut data, "rpc.timeout_ms", json!(5000)).unwrap();
        assert_eq!(data["rpc"]["timeout_ms"], 5000);
    }

    #[test]
    fn test_set_json_path_empty() {
        let mut data = json!({"foo": 42});
        assert_eq!(set_json_path(&mut data, "", json!(100)), Err(PathError::EmptyPath));
    }

    #[test]
    fn test_set_json_path_not_an_object() {
        let mut data = json!({"foo": 42});
        assert_eq!(
            set_json_path(&mut data, "foo.bar", json!(100)),
            Err(PathError::NotAnObject("foo".to_string()))
        );
    }
}
