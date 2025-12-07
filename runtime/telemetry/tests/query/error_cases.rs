//! Tests for error handling and invalid inputs.

use constellation_telemetry_service::query::parse;

#[test]
fn empty_query() {
    assert!(parse("").is_err());
}

#[test]
fn missing_braces() {
    assert!(parse("logs").is_err());
}

#[test]
fn unclosed_brace() {
    assert!(parse(r#"logs{service="auth""#).is_err());
}

#[test]
fn invalid_entry_type() {
    assert!(parse("unknown{}").is_err());
}

#[test]
fn missing_label_value() {
    assert!(parse("logs{service=}").is_err());
}

#[test]
fn unclosed_string() {
    assert!(parse(r#"logs{service="auth}"#).is_err());
}

#[test]
fn invalid_operator_in_label() {
    assert!(parse(r#"logs{service>"auth"}"#).is_err());
}

#[test]
fn empty_pipeline_stage() {
    assert!(parse("logs{} |").is_err());
}

#[test]
fn invalid_duration_unit() {
    assert!(parse("logs{} | last 5x").is_err());
}

#[test]
fn invalid_aggregation() {
    assert!(parse("logs{} | unknown_agg").is_err());
}

#[test]
fn rate_missing_duration() {
    assert!(parse("metrics{} | rate()").is_err());
}

#[test]
fn by_missing_fields() {
    assert!(parse("metrics{} | by()").is_err());
}

#[test]
fn invalid_timestamp_format() {
    assert!(parse("logs{} | from not-a-date to also-not-a-date").is_err());
}
