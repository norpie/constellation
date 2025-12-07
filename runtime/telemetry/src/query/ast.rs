//! Abstract Syntax Tree types for the query language.
//!
//! These types represent the parsed structure of a query, which can then be
//! translated to Datapad QueryBuilder calls.

use std::time::Duration;

// ============================================================================
// Top-Level Query
// ============================================================================

/// A complete parsed query.
///
/// Consists of a selector (what to query) and an optional pipeline of
/// transformations (filters, aggregations, time ranges).
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    /// The selector specifying entry type and label filters.
    pub selector: Selector,

    /// Pipeline stages to apply (filters, aggregations, time range).
    pub pipeline: Vec<PipelineStage>,
}

// ============================================================================
// Selector
// ============================================================================

/// Selector specifying what type of entries to query and label filters.
///
/// Examples:
/// - `logs{service="auth"}`
/// - `metrics{name="latency", env="prod"}`
/// - `*{trace_id="abc123"}`
#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    /// The entry type to query (or None for all types).
    pub entry_type: Option<EntryType>,

    /// Label matchers to filter entries.
    pub labels: Vec<LabelMatcher>,
}

/// Entry type discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    Log,
    Metric,
    Span,
}

// ============================================================================
// Label Matching
// ============================================================================

/// A label matcher for filtering entries.
///
/// Supports exact match, not equal, regex match, and regex not match.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelMatcher {
    /// The label key to match against.
    pub key: String,

    /// The match operation and value.
    pub matcher: MatchOp,
}

/// Label matching operations.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchOp {
    /// Exact string match (`=`).
    Equal(String),

    /// Not equal (`!=`).
    NotEqual(String),

    /// Regex match (`=~`).
    Regex(String),

    /// Regex not match (`!~`).
    NotRegex(String),
}

// ============================================================================
// Pipeline Stages
// ============================================================================

/// A stage in the query pipeline.
///
/// Stages are applied in order after the initial selection.
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineStage {
    /// Content filter for log messages (`|=` or `|~`).
    ContentFilter(ContentFilter),

    /// Field comparison filter (`level=error`, `duration > 100ms`).
    FieldFilter(FieldFilter),

    /// Aggregation function (`rate(5m)`, `avg`, `count`).
    Aggregation(Aggregation),

    /// Group by clause (`by(service, endpoint)`).
    GroupBy(Vec<String>),

    /// Time range restriction (`last 1h`, `from ... to ...`).
    TimeRange(TimeRange),
}

// ============================================================================
// Content Filters (for logs)
// ============================================================================

/// Content filter for searching within log messages.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentFilter {
    /// Contains substring (`|= "error"`).
    Contains(String),

    /// Matches regex (`|~ "user_\\d+"`).
    Regex(String),
}

// ============================================================================
// Field Filters
// ============================================================================

/// Field comparison filter.
///
/// Examples:
/// - `level=error`
/// - `duration > 100ms`
/// - `value >= 1000`
#[derive(Debug, Clone, PartialEq)]
pub struct FieldFilter {
    /// The field to compare.
    pub field: String,

    /// The comparison operation.
    pub op: CompareOp,

    /// The value to compare against.
    pub value: FieldValue,
}

/// Comparison operators for field filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    /// Equal (`=`).
    Eq,
    /// Not equal (`!=`).
    Ne,
    /// Greater than (`>`).
    Gt,
    /// Greater than or equal (`>=`).
    Ge,
    /// Less than (`<`).
    Lt,
    /// Less than or equal (`<=`).
    Le,
}

/// Value types for field comparisons.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    /// String value (e.g., `level=error`).
    String(String),

    /// Numeric value (e.g., `value > 1000`).
    Number(f64),

    /// Duration value (e.g., `duration > 100ms`).
    Duration(Duration),

    /// Log level (e.g., `level=error`).
    Level(LogLevel),
}

/// Log level for level filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

// ============================================================================
// Aggregations
// ============================================================================

/// Aggregation functions.
#[derive(Debug, Clone, PartialEq)]
pub enum Aggregation {
    /// Rate of change over a duration (`rate(5m)`).
    Rate(Duration),

    /// Average value.
    Avg,

    /// Sum of values.
    Sum,

    /// Minimum value.
    Min,

    /// Maximum value.
    Max,

    /// Count of entries.
    Count,

    /// 50th percentile.
    P50,

    /// 90th percentile.
    P90,

    /// 95th percentile.
    P95,

    /// 99th percentile.
    P99,
}

// ============================================================================
// Time Ranges
// ============================================================================

/// Time range for filtering entries.
#[derive(Debug, Clone, PartialEq)]
pub enum TimeRange {
    /// Relative time range (`last 1h`, `last 30m`).
    Last(Duration),

    /// Absolute time range (`from ... to ...`).
    /// Timestamps are in microseconds since Unix epoch.
    Absolute {
        from: u64,
        to: u64,
    },
}

// ============================================================================
// Conversions
// ============================================================================

impl From<LogLevel> for constellation_telemetry::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Debug => constellation_telemetry::Level::Debug,
            LogLevel::Info => constellation_telemetry::Level::Info,
            LogLevel::Warn => constellation_telemetry::Level::Warn,
            LogLevel::Error => constellation_telemetry::Level::Error,
        }
    }
}

impl From<EntryType> for constellation_telemetry::EntryType {
    fn from(et: EntryType) -> Self {
        match et {
            EntryType::Log => constellation_telemetry::EntryType::Log,
            EntryType::Metric => constellation_telemetry::EntryType::Metric,
            EntryType::Span => constellation_telemetry::EntryType::Span,
        }
    }
}
