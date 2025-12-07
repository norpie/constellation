//! Tests for content filters (|= and |~ for log message searching).

use constellation_telemetry_service::query::*;

#[test]
fn contains_string() {
    let q = parse(r#"logs{} |= "error""#).unwrap();
    assert_eq!(q.pipeline.len(), 1);
    assert_eq!(
        q.pipeline[0],
        PipelineStage::ContentFilter(ContentFilter::Contains("error".into()))
    );
}

#[test]
fn regex_match() {
    let q = parse(r#"logs{} |~ "user_\d+""#).unwrap();
    assert_eq!(q.pipeline.len(), 1);
    assert_eq!(
        q.pipeline[0],
        PipelineStage::ContentFilter(ContentFilter::Regex(r"user_\d+".into()))
    );
}

#[test]
fn content_filter_with_selector_labels() {
    let q = parse(r#"logs{service="auth"} |= "failed""#).unwrap();
    assert_eq!(q.selector.labels.len(), 1);
    assert_eq!(q.pipeline.len(), 1);
    assert_eq!(
        q.pipeline[0],
        PipelineStage::ContentFilter(ContentFilter::Contains("failed".into()))
    );
}

#[test]
fn multiple_content_filters() {
    let q = parse(r#"logs{} |= "error" |= "timeout""#).unwrap();
    assert_eq!(q.pipeline.len(), 2);
    assert_eq!(
        q.pipeline[0],
        PipelineStage::ContentFilter(ContentFilter::Contains("error".into()))
    );
    assert_eq!(
        q.pipeline[1],
        PipelineStage::ContentFilter(ContentFilter::Contains("timeout".into()))
    );
}
