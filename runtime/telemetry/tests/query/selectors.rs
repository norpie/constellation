//! Tests for selector parsing (entry types and basic label matching).

use constellation_telemetry_service::query::*;

#[test]
fn logs_empty_labels() {
    let q = parse("logs{}").unwrap();
    assert_eq!(q.selector.entry_type, Some(EntryType::Log));
    assert!(q.selector.labels.is_empty());
    assert!(q.pipeline.is_empty());
}

#[test]
fn metrics_empty_labels() {
    let q = parse("metrics{}").unwrap();
    assert_eq!(q.selector.entry_type, Some(EntryType::Metric));
    assert!(q.selector.labels.is_empty());
}

#[test]
fn spans_empty_labels() {
    let q = parse("spans{}").unwrap();
    assert_eq!(q.selector.entry_type, Some(EntryType::Span));
    assert!(q.selector.labels.is_empty());
}

#[test]
fn wildcard_selector() {
    let q = parse("*{}").unwrap();
    assert_eq!(q.selector.entry_type, None);
    assert!(q.selector.labels.is_empty());
}

#[test]
fn logs_with_single_label() {
    let q = parse(r#"logs{service="auth"}"#).unwrap();
    assert_eq!(q.selector.entry_type, Some(EntryType::Log));
    assert_eq!(q.selector.labels.len(), 1);
    assert_eq!(q.selector.labels[0].key, "service");
    assert_eq!(q.selector.labels[0].matcher, MatchOp::Equal("auth".into()));
}

#[test]
fn metrics_with_name_label() {
    let q = parse(r#"metrics{name="request_duration"}"#).unwrap();
    assert_eq!(q.selector.entry_type, Some(EntryType::Metric));
    assert_eq!(q.selector.labels.len(), 1);
    assert_eq!(q.selector.labels[0].key, "name");
    assert_eq!(
        q.selector.labels[0].matcher,
        MatchOp::Equal("request_duration".into())
    );
}

#[test]
fn multiple_labels() {
    let q = parse(r#"logs{service="auth", env="prod"}"#).unwrap();
    assert_eq!(q.selector.labels.len(), 2);
    assert_eq!(q.selector.labels[0].key, "service");
    assert_eq!(q.selector.labels[1].key, "env");
}

#[test]
fn wildcard_with_trace_id() {
    let q = parse(r#"*{trace_id="abc123def456"}"#).unwrap();
    assert_eq!(q.selector.entry_type, None);
    assert_eq!(q.selector.labels.len(), 1);
    assert_eq!(q.selector.labels[0].key, "trace_id");
    assert_eq!(
        q.selector.labels[0].matcher,
        MatchOp::Equal("abc123def456".into())
    );
}

#[test]
fn whitespace_tolerance() {
    let q = parse(r#"logs{ service = "auth" }"#).unwrap();
    assert_eq!(q.selector.entry_type, Some(EntryType::Log));
    assert_eq!(q.selector.labels[0].key, "service");
}
