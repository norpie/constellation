//! Query executor that translates parsed AST to QueryBuilder calls.
//!
//! This module bridges the query language parser with the Datapad QueryBuilder,
//! translating AST nodes to appropriate builder method calls.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use constellation_datapad::Datapad;
use constellation_telemetry::{Level, TelemetryEntry};
use thiserror::Error;

use super::ast::*;
use super::parser::ParseError;

/// Error type for query execution failures.
#[derive(Debug, Error)]
pub enum ExecuteError {
    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("Datapad error: {0}")]
    Datapad(#[from] constellation_datapad::error::Error),

    #[error("Unsupported feature: {0}")]
    Unsupported(String),

    #[error("Invalid query: {0}")]
    Invalid(String),
}

/// Result of query execution.
pub struct QueryResult {
    /// The matching entries.
    pub entries: Vec<TelemetryEntry>,
}

/// Execute a parsed query against a Datapad.
pub fn execute(datapad: &Datapad, query: &Query) -> Result<QueryResult, ExecuteError> {
    let mut builder = datapad.query();

    // Apply entry type filter
    if let Some(entry_type) = query.selector.entry_type {
        builder = builder.entry_type(entry_type.into());
    }

    // Apply label matchers
    for label in &query.selector.labels {
        builder = apply_label_matcher(builder, label)?;
    }

    // Process pipeline stages for time range (apply to builder)
    for stage in &query.pipeline {
        if let PipelineStage::TimeRange(time_range) = stage {
            builder = apply_time_range(builder, time_range)?;
        }
    }

    // Execute the base query
    let mut entries = builder.execute()?;

    // Apply post-filters for pipeline stages not supported by QueryBuilder
    for stage in &query.pipeline {
        entries = apply_pipeline_stage(entries, stage)?;
    }

    Ok(QueryResult { entries })
}

/// Execute a query string against a Datapad.
pub fn execute_query(datapad: &Datapad, query_str: &str) -> Result<QueryResult, ExecuteError> {
    let query = super::parse(query_str)?;
    execute(datapad, &query)
}

/// Apply a label matcher to the query builder.
fn apply_label_matcher<'a>(
    builder: constellation_datapad::query::QueryBuilder<'a>,
    label: &LabelMatcher,
) -> Result<constellation_datapad::query::QueryBuilder<'a>, ExecuteError> {
    match &label.matcher {
        MatchOp::Equal(value) => {
            // Map known labels to QueryBuilder methods
            match label.key.as_str() {
                "service" => Ok(builder.service(value)),
                "trace_id" => Ok(builder.trace_id(value)),
                "name" => Ok(builder.metric_name(value)),
                // Everything else becomes a tag filter
                key => Ok(builder.tag(key, value)),
            }
        }
        MatchOp::NotEqual(_) => Err(ExecuteError::Unsupported(
            "!= operator in label matchers (not supported by QueryBuilder)".into(),
        )),
        MatchOp::Regex(_) => Err(ExecuteError::Unsupported(
            "=~ operator in label matchers (regex not supported by QueryBuilder)".into(),
        )),
        MatchOp::NotRegex(_) => Err(ExecuteError::Unsupported(
            "!~ operator in label matchers (regex not supported by QueryBuilder)".into(),
        )),
    }
}

/// Apply a time range to the query builder.
fn apply_time_range<'a>(
    builder: constellation_datapad::query::QueryBuilder<'a>,
    time_range: &TimeRange,
) -> Result<constellation_datapad::query::QueryBuilder<'a>, ExecuteError> {
    match time_range {
        TimeRange::Last(duration) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64;
            let start = now.saturating_sub(duration.as_micros() as u64);
            Ok(builder.time_range(start, now))
        }
        TimeRange::Absolute { from, to } => Ok(builder.time_range(*from, *to)),
    }
}

/// Apply a pipeline stage as a post-filter.
fn apply_pipeline_stage(
    entries: Vec<TelemetryEntry>,
    stage: &PipelineStage,
) -> Result<Vec<TelemetryEntry>, ExecuteError> {
    match stage {
        // Time range already applied to builder
        PipelineStage::TimeRange(_) => Ok(entries),

        // Field filters
        PipelineStage::FieldFilter(filter) => apply_field_filter(entries, filter),

        // Content filters (for logs)
        PipelineStage::ContentFilter(filter) => apply_content_filter(entries, filter),

        // Aggregations not yet supported
        PipelineStage::Aggregation(agg) => Err(ExecuteError::Unsupported(format!(
            "Aggregation {:?} not yet implemented",
            agg
        ))),

        // Group by not yet supported
        PipelineStage::GroupBy(fields) => Err(ExecuteError::Unsupported(format!(
            "Group by {:?} not yet implemented",
            fields
        ))),
    }
}

/// Apply a field filter as a post-filter.
fn apply_field_filter(
    entries: Vec<TelemetryEntry>,
    filter: &FieldFilter,
) -> Result<Vec<TelemetryEntry>, ExecuteError> {
    let filtered = entries
        .into_iter()
        .filter(|entry| matches_field_filter(entry, filter))
        .collect();
    Ok(filtered)
}

/// Check if an entry matches a field filter.
fn matches_field_filter(entry: &TelemetryEntry, filter: &FieldFilter) -> bool {
    match filter.field.as_str() {
        "level" => match entry {
            TelemetryEntry::Log(log) => {
                if let FieldValue::Level(level) = &filter.value {
                    let entry_level: Level = (*level).into();
                    match filter.op {
                        CompareOp::Eq => log.level == entry_level,
                        CompareOp::Ne => log.level != entry_level,
                        // Levels can be compared (debug < info < warn < error)
                        CompareOp::Gt => log.level.as_u8() > entry_level.as_u8(),
                        CompareOp::Ge => log.level.as_u8() >= entry_level.as_u8(),
                        CompareOp::Lt => log.level.as_u8() < entry_level.as_u8(),
                        CompareOp::Le => log.level.as_u8() <= entry_level.as_u8(),
                    }
                } else {
                    false
                }
            }
            _ => true, // Non-logs pass level filter
        },

        "duration" => match entry {
            TelemetryEntry::Span(span) => {
                if let FieldValue::Duration(duration) = &filter.value {
                    // Duration is end - start (both in microseconds)
                    let span_duration_us = span.end.saturating_sub(span.start);
                    let span_duration = Duration::from_micros(span_duration_us);
                    match filter.op {
                        CompareOp::Eq => span_duration == *duration,
                        CompareOp::Ne => span_duration != *duration,
                        CompareOp::Gt => span_duration > *duration,
                        CompareOp::Ge => span_duration >= *duration,
                        CompareOp::Lt => span_duration < *duration,
                        CompareOp::Le => span_duration <= *duration,
                    }
                } else {
                    false
                }
            }
            _ => true, // Non-spans pass duration filter
        },

        "value" => match entry {
            TelemetryEntry::Metric(metric) => {
                if let FieldValue::Number(n) = &filter.value {
                    match filter.op {
                        CompareOp::Eq => (metric.value - n).abs() < f64::EPSILON,
                        CompareOp::Ne => (metric.value - n).abs() >= f64::EPSILON,
                        CompareOp::Gt => metric.value > *n,
                        CompareOp::Ge => metric.value >= *n,
                        CompareOp::Lt => metric.value < *n,
                        CompareOp::Le => metric.value <= *n,
                    }
                } else {
                    false
                }
            }
            _ => true, // Non-metrics pass value filter
        },

        // Generic tag comparison
        key => {
            let tags = &entry.common().tags;
            if let Some(tag_value) = tags.get(key) {
                match &filter.value {
                    FieldValue::String(s) => match filter.op {
                        CompareOp::Eq => tag_value == s,
                        CompareOp::Ne => tag_value != s,
                        _ => false, // String comparisons only support = and !=
                    },
                    _ => false,
                }
            } else {
                // Tag doesn't exist
                matches!(filter.op, CompareOp::Ne)
            }
        }
    }
}

/// Apply a content filter (for logs).
fn apply_content_filter(
    entries: Vec<TelemetryEntry>,
    filter: &ContentFilter,
) -> Result<Vec<TelemetryEntry>, ExecuteError> {
    let filtered = entries
        .into_iter()
        .filter(|entry| matches_content_filter(entry, filter))
        .collect();
    Ok(filtered)
}

/// Check if an entry matches a content filter.
fn matches_content_filter(entry: &TelemetryEntry, filter: &ContentFilter) -> bool {
    match entry {
        TelemetryEntry::Log(log) => match filter {
            ContentFilter::Contains(s) => log.message.contains(s),
            ContentFilter::Regex(pattern) => {
                // Simple regex matching - in production, we'd cache compiled regexes
                regex::Regex::new(pattern)
                    .map(|re| re.is_match(&log.message))
                    .unwrap_or(false)
            }
        },
        // Non-logs pass content filters (or should they fail?)
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_range_last() {
        let datapad = Datapad::open_temporary().unwrap();
        let query = super::super::parse("logs{} | last 1h").unwrap();
        let result = execute(&datapad, &query);
        assert!(result.is_ok());
    }

    #[test]
    fn test_unsupported_aggregation() {
        let datapad = Datapad::open_temporary().unwrap();
        let query = super::super::parse("logs{} | count").unwrap();
        let result = execute(&datapad, &query);
        assert!(matches!(result, Err(ExecuteError::Unsupported(_))));
    }
}
