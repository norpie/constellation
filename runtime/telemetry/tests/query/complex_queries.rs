//! Tests for complex queries combining multiple features.

use constellation_telemetry_service::query::*;
use std::time::Duration;

#[test]
fn error_logs_last_hour() {
    let q = parse(r#"logs{service="auth"} | level=error | last 1h"#).unwrap();

    assert_eq!(q.selector.entry_type, Some(EntryType::Log));
    assert_eq!(q.selector.labels.len(), 1);
    assert_eq!(q.pipeline.len(), 2);

    match &q.pipeline[0] {
        PipelineStage::FieldFilter(f) => {
            assert_eq!(f.field, "level");
            assert_eq!(f.value, FieldValue::Level(LogLevel::Error));
        }
        _ => panic!("Expected FieldFilter"),
    }

    assert_eq!(
        q.pipeline[1],
        PipelineStage::TimeRange(TimeRange::Last(Duration::from_secs(3600)))
    );
}

#[test]
fn slow_spans_by_endpoint() {
    let q = parse(r#"spans{service="api"} | duration > 500ms | by(endpoint) | last 30m"#).unwrap();

    assert_eq!(q.selector.entry_type, Some(EntryType::Span));
    assert_eq!(q.pipeline.len(), 3);

    match &q.pipeline[0] {
        PipelineStage::FieldFilter(f) => {
            assert_eq!(f.field, "duration");
            assert_eq!(f.op, CompareOp::Gt);
            assert_eq!(f.value, FieldValue::Duration(Duration::from_millis(500)));
        }
        _ => panic!("Expected FieldFilter"),
    }

    assert_eq!(
        q.pipeline[1],
        PipelineStage::GroupBy(vec!["endpoint".into()])
    );

    assert_eq!(
        q.pipeline[2],
        PipelineStage::TimeRange(TimeRange::Last(Duration::from_secs(30 * 60)))
    );
}

#[test]
fn p99_latency_by_service() {
    let q = parse(r#"metrics{name="request_duration"} | p99 | by(service) | last 1h"#).unwrap();

    assert_eq!(q.selector.entry_type, Some(EntryType::Metric));
    assert_eq!(q.selector.labels.len(), 1);
    assert_eq!(q.pipeline.len(), 3);

    assert_eq!(q.pipeline[0], PipelineStage::Aggregation(Aggregation::P99));
    assert_eq!(
        q.pipeline[1],
        PipelineStage::GroupBy(vec!["service".into()])
    );
}

#[test]
fn error_rate_by_service() {
    let q = parse(r#"metrics{name="errors_total"} | rate(5m) | by(service) | last 1h"#).unwrap();

    assert_eq!(q.pipeline.len(), 3);
    assert_eq!(
        q.pipeline[0],
        PipelineStage::Aggregation(Aggregation::Rate(Duration::from_secs(5 * 60)))
    );
    assert_eq!(
        q.pipeline[1],
        PipelineStage::GroupBy(vec!["service".into()])
    );
}

#[test]
fn logs_with_content_and_level_filter() {
    let q = parse(r#"logs{service="auth"} |= "failed" | level=error | last 15m"#).unwrap();

    assert_eq!(q.pipeline.len(), 3);
    assert_eq!(
        q.pipeline[0],
        PipelineStage::ContentFilter(ContentFilter::Contains("failed".into()))
    );

    match &q.pipeline[1] {
        PipelineStage::FieldFilter(f) => {
            assert_eq!(f.field, "level");
            assert_eq!(f.value, FieldValue::Level(LogLevel::Error));
        }
        _ => panic!("Expected FieldFilter"),
    }
}

#[test]
fn count_by_level() {
    let q = parse(r#"logs{service="auth"} | count | by(level)"#).unwrap();

    assert_eq!(q.pipeline.len(), 2);
    assert_eq!(
        q.pipeline[0],
        PipelineStage::Aggregation(Aggregation::Count)
    );
    assert_eq!(q.pipeline[1], PipelineStage::GroupBy(vec!["level".into()]));
}

#[test]
fn trace_debugging_all_types() {
    let q = parse(r#"*{trace_id="abc123def456"}"#).unwrap();

    assert_eq!(q.selector.entry_type, None);
    assert_eq!(q.selector.labels.len(), 1);
    assert_eq!(q.selector.labels[0].key, "trace_id");
    assert!(q.pipeline.is_empty());
}

#[test]
fn high_cardinality_drill_down() {
    let q = parse(r#"logs{service="auth", user_id="12345"} | last 24h"#).unwrap();

    assert_eq!(q.selector.labels.len(), 2);
    assert_eq!(q.selector.labels[0].key, "service");
    assert_eq!(q.selector.labels[1].key, "user_id");
    assert_eq!(
        q.pipeline[0],
        PipelineStage::TimeRange(TimeRange::Last(Duration::from_secs(24 * 3600)))
    );
}
