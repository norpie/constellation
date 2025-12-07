//! Tests for time range specifications (last, from/to).

use constellation_telemetry_service::query::*;
use std::time::Duration;

#[test]
fn last_seconds() {
    let q = parse(r#"logs{} | last 30s"#).unwrap();
    assert_eq!(
        q.pipeline[0],
        PipelineStage::TimeRange(TimeRange::Last(Duration::from_secs(30)))
    );
}

#[test]
fn last_minutes() {
    let q = parse(r#"logs{} | last 15m"#).unwrap();
    assert_eq!(
        q.pipeline[0],
        PipelineStage::TimeRange(TimeRange::Last(Duration::from_secs(15 * 60)))
    );
}

#[test]
fn last_hours() {
    let q = parse(r#"logs{} | last 1h"#).unwrap();
    assert_eq!(
        q.pipeline[0],
        PipelineStage::TimeRange(TimeRange::Last(Duration::from_secs(3600)))
    );
}

#[test]
fn last_days() {
    let q = parse(r#"logs{} | last 7d"#).unwrap();
    assert_eq!(
        q.pipeline[0],
        PipelineStage::TimeRange(TimeRange::Last(Duration::from_secs(7 * 24 * 3600)))
    );
}

#[test]
fn absolute_range_rfc3339() {
    let q = parse(r#"logs{} | from 2024-01-01T00:00:00Z to 2024-01-02T00:00:00Z"#).unwrap();
    match &q.pipeline[0] {
        PipelineStage::TimeRange(TimeRange::Absolute { from, to }) => {
            // 2024-01-01T00:00:00Z in microseconds
            assert_eq!(*from, 1704067200_000_000);
            // 2024-01-02T00:00:00Z in microseconds
            assert_eq!(*to, 1704153600_000_000);
        }
        _ => panic!("Expected Absolute TimeRange"),
    }
}
