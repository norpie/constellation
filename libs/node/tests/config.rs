//! Tests for config module

use constellation_node::config::{get_json_path, set_json_path, PathError};
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
