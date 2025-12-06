use serde::{Deserialize, Serialize};

use super::common::{CommonFields, SpanId, Timestamp};

/// Span completion status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpanStatus {
    /// Span completed successfully
    Ok,
    /// Span completed with an error
    Error,
}

impl SpanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanStatus::Ok => "ok",
            SpanStatus::Error => "error",
        }
    }
}

impl Default for SpanStatus {
    fn default() -> Self {
        Self::Ok
    }
}

impl std::fmt::Display for SpanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A span entry representing a unit of work in a trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEntry {
    /// Common fields (id, timestamp, service, node_id, trace_id, span_id, tags)
    #[serde(flatten)]
    pub common: CommonFields,

    /// Operation name (e.g., "handle_login", "db_query")
    pub name: String,

    /// Parent span ID (None for root spans)
    pub parent_span_id: Option<SpanId>,

    /// Start timestamp in microseconds
    pub start: Timestamp,

    /// End timestamp in microseconds
    pub end: Timestamp,

    /// Completion status
    pub status: SpanStatus,
}

impl SpanEntry {
    /// Create a new span entry
    pub fn new(
        common: CommonFields,
        name: impl Into<String>,
        start: Timestamp,
        end: Timestamp,
    ) -> Self {
        Self {
            common,
            name: name.into(),
            parent_span_id: None,
            start,
            end,
            status: SpanStatus::Ok,
        }
    }

    /// Set parent span ID
    pub fn with_parent(mut self, parent_span_id: SpanId) -> Self {
        self.parent_span_id = Some(parent_span_id);
        self
    }

    /// Set status
    pub fn with_status(mut self, status: SpanStatus) -> Self {
        self.status = status;
        self
    }

    /// Mark span as errored
    pub fn with_error(mut self) -> Self {
        self.status = SpanStatus::Error;
        self
    }

    /// Get span duration in microseconds
    pub fn duration_micros(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Get span duration in milliseconds
    pub fn duration_ms(&self) -> f64 {
        self.duration_micros() as f64 / 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_creation() {
        let common = CommonFields::new("api", "api-1");
        let span = SpanEntry::new(common, "handle_request", 1000000, 1500000);

        assert_eq!(span.name, "handle_request");
        assert_eq!(span.start, 1000000);
        assert_eq!(span.end, 1500000);
        assert_eq!(span.status, SpanStatus::Ok);
        assert!(span.parent_span_id.is_none());
    }

    #[test]
    fn span_with_parent() {
        let common = CommonFields::new("api", "api-1");
        let parent_id = SpanId::new();
        let span = SpanEntry::new(common, "db_query", 1000000, 1200000).with_parent(parent_id);

        assert!(span.parent_span_id.is_some());
    }

    #[test]
    fn span_duration() {
        let common = CommonFields::new("api", "api-1");
        let span = SpanEntry::new(common, "operation", 1000000, 1500000);

        assert_eq!(span.duration_micros(), 500000);
        assert_eq!(span.duration_ms(), 500.0);
    }

    #[test]
    fn span_with_error() {
        let common = CommonFields::new("api", "api-1");
        let span = SpanEntry::new(common, "failed_operation", 1000000, 1100000).with_error();

        assert_eq!(span.status, SpanStatus::Error);
    }
}
