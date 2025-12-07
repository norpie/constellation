//! Tests for aggregation functions (rate, avg, sum, count, percentiles, by).

use constellation_telemetry_service::query::*;
use std::time::Duration;

#[test]
fn rate_with_duration() {
    let q = parse(r#"metrics{} | rate(5m)"#).unwrap();
    assert_eq!(
        q.pipeline[0],
        PipelineStage::Aggregation(Aggregation::Rate(Duration::from_secs(5 * 60)))
    );
}

#[test]
fn rate_with_seconds() {
    let q = parse(r#"metrics{} | rate(30s)"#).unwrap();
    assert_eq!(
        q.pipeline[0],
        PipelineStage::Aggregation(Aggregation::Rate(Duration::from_secs(30)))
    );
}

#[test]
fn rate_with_hours() {
    let q = parse(r#"metrics{} | rate(1h)"#).unwrap();
    assert_eq!(
        q.pipeline[0],
        PipelineStage::Aggregation(Aggregation::Rate(Duration::from_secs(3600)))
    );
}

#[test]
fn avg_aggregation() {
    let q = parse(r#"metrics{} | avg"#).unwrap();
    assert_eq!(q.pipeline[0], PipelineStage::Aggregation(Aggregation::Avg));
}

#[test]
fn sum_aggregation() {
    let q = parse(r#"metrics{} | sum"#).unwrap();
    assert_eq!(q.pipeline[0], PipelineStage::Aggregation(Aggregation::Sum));
}

#[test]
fn min_aggregation() {
    let q = parse(r#"metrics{} | min"#).unwrap();
    assert_eq!(q.pipeline[0], PipelineStage::Aggregation(Aggregation::Min));
}

#[test]
fn max_aggregation() {
    let q = parse(r#"metrics{} | max"#).unwrap();
    assert_eq!(q.pipeline[0], PipelineStage::Aggregation(Aggregation::Max));
}

#[test]
fn count_aggregation() {
    let q = parse(r#"logs{} | count"#).unwrap();
    assert_eq!(
        q.pipeline[0],
        PipelineStage::Aggregation(Aggregation::Count)
    );
}

#[test]
fn p50_percentile() {
    let q = parse(r#"metrics{} | p50"#).unwrap();
    assert_eq!(q.pipeline[0], PipelineStage::Aggregation(Aggregation::P50));
}

#[test]
fn p90_percentile() {
    let q = parse(r#"metrics{} | p90"#).unwrap();
    assert_eq!(q.pipeline[0], PipelineStage::Aggregation(Aggregation::P90));
}

#[test]
fn p95_percentile() {
    let q = parse(r#"metrics{} | p95"#).unwrap();
    assert_eq!(q.pipeline[0], PipelineStage::Aggregation(Aggregation::P95));
}

#[test]
fn p99_percentile() {
    let q = parse(r#"metrics{} | p99"#).unwrap();
    assert_eq!(q.pipeline[0], PipelineStage::Aggregation(Aggregation::P99));
}

#[test]
fn group_by_single_field() {
    let q = parse(r#"metrics{} | by(service)"#).unwrap();
    assert_eq!(
        q.pipeline[0],
        PipelineStage::GroupBy(vec!["service".into()])
    );
}

#[test]
fn group_by_multiple_fields() {
    let q = parse(r#"metrics{} | by(service, endpoint, method)"#).unwrap();
    assert_eq!(
        q.pipeline[0],
        PipelineStage::GroupBy(vec![
            "service".into(),
            "endpoint".into(),
            "method".into()
        ])
    );
}

#[test]
fn aggregation_with_group_by() {
    let q = parse(r#"metrics{} | sum | by(service)"#).unwrap();
    assert_eq!(q.pipeline.len(), 2);
    assert_eq!(q.pipeline[0], PipelineStage::Aggregation(Aggregation::Sum));
    assert_eq!(
        q.pipeline[1],
        PipelineStage::GroupBy(vec!["service".into()])
    );
}
