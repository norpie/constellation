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
    /// Placeholder - will be replaced with real config sections
    #[default = 42]
    pub placeholder: u64,
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
