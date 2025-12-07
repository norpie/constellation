//! Tests for field filters (level=error, duration > 100ms, etc.).

use constellation_telemetry_service::query::*;
use std::time::Duration;

#[test]
fn level_equals() {
    let q = parse(r#"logs{} | level=error"#).unwrap();
    assert_eq!(q.pipeline.len(), 1);
    match &q.pipeline[0] {
        PipelineStage::FieldFilter(f) => {
            assert_eq!(f.field, "level");
            assert_eq!(f.op, CompareOp::Eq);
            assert_eq!(f.value, FieldValue::Level(LogLevel::Error));
        }
        _ => panic!("Expected FieldFilter"),
    }
}

#[test]
fn level_not_equals() {
    let q = parse(r#"logs{} | level!=debug"#).unwrap();
    match &q.pipeline[0] {
        PipelineStage::FieldFilter(f) => {
            assert_eq!(f.op, CompareOp::Ne);
            assert_eq!(f.value, FieldValue::Level(LogLevel::Debug));
        }
        _ => panic!("Expected FieldFilter"),
    }
}

#[test]
fn duration_greater_than() {
    let q = parse(r#"spans{} | duration > 100ms"#).unwrap();
    match &q.pipeline[0] {
        PipelineStage::FieldFilter(f) => {
            assert_eq!(f.field, "duration");
            assert_eq!(f.op, CompareOp::Gt);
            assert_eq!(f.value, FieldValue::Duration(Duration::from_millis(100)));
        }
        _ => panic!("Expected FieldFilter"),
    }
}

#[test]
fn duration_greater_than_or_equal() {
    let q = parse(r#"spans{} | duration >= 1s"#).unwrap();
    match &q.pipeline[0] {
        PipelineStage::FieldFilter(f) => {
            assert_eq!(f.op, CompareOp::Ge);
            assert_eq!(f.value, FieldValue::Duration(Duration::from_secs(1)));
        }
        _ => panic!("Expected FieldFilter"),
    }
}

#[test]
fn value_less_than() {
    let q = parse(r#"metrics{} | value < 1000"#).unwrap();
    match &q.pipeline[0] {
        PipelineStage::FieldFilter(f) => {
            assert_eq!(f.field, "value");
            assert_eq!(f.op, CompareOp::Lt);
            assert_eq!(f.value, FieldValue::Number(1000.0));
        }
        _ => panic!("Expected FieldFilter"),
    }
}

#[test]
fn value_less_than_or_equal() {
    let q = parse(r#"metrics{} | value <= 500.5"#).unwrap();
    match &q.pipeline[0] {
        PipelineStage::FieldFilter(f) => {
            assert_eq!(f.op, CompareOp::Le);
            assert_eq!(f.value, FieldValue::Number(500.5));
        }
        _ => panic!("Expected FieldFilter"),
    }
}

#[test]
fn string_field_filter() {
    let q = parse(r#"spans{} | endpoint="/api/users""#).unwrap();
    match &q.pipeline[0] {
        PipelineStage::FieldFilter(f) => {
            assert_eq!(f.field, "endpoint");
            assert_eq!(f.op, CompareOp::Eq);
            assert_eq!(f.value, FieldValue::String("/api/users".into()));
        }
        _ => panic!("Expected FieldFilter"),
    }
}
