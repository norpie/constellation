//! Span management for tracing operations.
//!
//! Spans represent units of work within a trace. They have a start time,
//! end time, and can be nested to form a tree structure.

use super::context::TelemetryContext;
use constellation_telemetry::{
    current_timestamp_micros, Collector, CommonFields, SpanEntry, SpanId, SpanStatus, Timestamp,
};
use std::collections::HashMap;

/// A span representing a unit of work.
///
/// Spans are created with `Span::enter()` and automatically record their
/// duration when dropped (via `SpanGuard`).
///
/// # Example
///
/// ```ignore
/// let _guard = Span::enter("database_query");
/// // ... do work ...
/// // span is automatically ended when _guard is dropped
/// ```
pub struct Span {
    /// Span name (operation name)
    name: String,

    /// Start timestamp in microseconds
    start: Timestamp,

    /// Tags to attach to the span
    tags: HashMap<String, String>,

    /// Completion status
    status: SpanStatus,

    /// The span ID assigned when entering
    span_id: Option<SpanId>,

    /// Parent span ID (captured at creation time)
    parent_span_id: Option<SpanId>,

    /// Whether this span has been recorded
    recorded: bool,
}

impl Span {
    /// Create a new span but don't enter it yet.
    ///
    /// Call `enter()` to start the span and get a guard.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            start: current_timestamp_micros(),
            tags: HashMap::new(),
            status: SpanStatus::Ok,
            span_id: None,
            parent_span_id: None,
            recorded: false,
        }
    }

    /// Create and enter a span, returning a guard.
    ///
    /// The span is automatically ended when the guard is dropped.
    /// If no telemetry context is active, returns a no-op guard.
    pub fn enter(name: impl Into<String>) -> SpanGuard {
        let mut span = Self::new(name);

        // Try to push onto context stack
        let entered = TelemetryContext::with_mut(|ctx| {
            // Capture current span as parent BEFORE pushing new span
            span.parent_span_id = Some(ctx.span_id.clone());
            span.span_id = Some(ctx.push_span());
        });

        SpanGuard {
            span: Some(span),
            active: entered.is_some(),
        }
    }

    /// Add a tag to the span.
    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Mark the span as errored.
    pub fn error(mut self) -> Self {
        self.status = SpanStatus::Error;
        self
    }

    /// Set the span status.
    pub fn status(mut self, status: SpanStatus) -> Self {
        self.status = status;
        self
    }

    /// Manually end the span and record it.
    ///
    /// This is called automatically by `SpanGuard::drop()`.
    pub fn end(mut self) {
        self.record();
    }

    /// Record the span to the collector.
    fn record(&mut self) {
        if self.recorded {
            return;
        }
        self.recorded = true;

        let end = current_timestamp_micros();

        // Get context and record span
        TelemetryContext::with(|ctx| {
            let mut common = CommonFields::new(&ctx.service, &ctx.node_id)
                .with_trace_id(ctx.trace_id.clone());

            if let Some(ref span_id) = self.span_id {
                common = common.with_span_id(span_id.clone());
            }

            for (k, v) in &self.tags {
                common = common.with_tag(k.clone(), v.clone());
            }

            let mut entry = SpanEntry::new(common, &self.name, self.start, end)
                .with_status(self.status);

            if let Some(ref parent) = self.parent_span_id {
                entry = entry.with_parent(parent.clone());
            }

            ctx.collector.collect_span(entry);
        });

        // Pop from context stack
        TelemetryContext::with_mut(|ctx| {
            ctx.pop_span();
        });
    }
}

/// Guard that ends a span when dropped.
///
/// This enables RAII-style span management where spans are automatically
/// ended when they go out of scope.
pub struct SpanGuard {
    span: Option<Span>,
    active: bool,
}

impl SpanGuard {
    /// Create a no-op guard (for when no context is active).
    pub fn noop() -> Self {
        Self {
            span: None,
            active: false,
        }
    }

    /// Check if this guard is active (has a real span).
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Mark the span as errored.
    pub fn set_error(&mut self) {
        if let Some(ref mut span) = self.span {
            span.status = SpanStatus::Error;
        }
    }

    /// Add a tag to the span.
    pub fn set_tag(&mut self, key: impl Into<String>, value: impl Into<String>) {
        if let Some(ref mut span) = self.span {
            span.tags.insert(key.into(), value.into());
        }
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        if let Some(mut span) = self.span.take() {
            span.record();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Data;
    use constellation_telemetry::{BufferCollector, CollectorConfig, TelemetryEntry};

    fn make_collector() -> Data<BufferCollector> {
        Data::new(BufferCollector::new(CollectorConfig::default()))
    }

    #[tokio::test]
    async fn span_enter_and_drop() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            {
                let _guard = Span::enter("test_operation");
                // Span is active
                assert!(TelemetryContext::with(|ctx| ctx.span_depth()) == Some(2));
            }
            // Guard dropped, span should be recorded and popped
            assert!(TelemetryContext::with(|ctx| ctx.span_depth()) == Some(1));
        })
        .await;

        // Check that span was collected
        let entries = collector.drain();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            TelemetryEntry::Span(span) => {
                assert_eq!(span.name, "test_operation");
                assert_eq!(span.status, SpanStatus::Ok);
            }
            _ => panic!("Expected span entry"),
        }
    }

    #[tokio::test]
    async fn nested_spans() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            {
                let _outer = Span::enter("outer");
                assert_eq!(TelemetryContext::with(|ctx| ctx.span_depth()), Some(2));

                {
                    let _inner = Span::enter("inner");
                    assert_eq!(TelemetryContext::with(|ctx| ctx.span_depth()), Some(3));
                }

                assert_eq!(TelemetryContext::with(|ctx| ctx.span_depth()), Some(2));
            }
            assert_eq!(TelemetryContext::with(|ctx| ctx.span_depth()), Some(1));
        })
        .await;

        // Both spans should be collected (inner first due to drop order)
        let entries = collector.drain();
        assert_eq!(entries.len(), 2);

        let names: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                TelemetryEntry::Span(s) => Some(s.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["inner", "outer"]);
    }

    #[tokio::test]
    async fn span_with_error() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            {
                let mut guard = Span::enter("failing_operation");
                guard.set_error();
            }
        })
        .await;

        let entries = collector.drain();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            TelemetryEntry::Span(span) => {
                assert_eq!(span.status, SpanStatus::Error);
            }
            _ => panic!("Expected span entry"),
        }
    }

    #[tokio::test]
    async fn span_with_tags() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            {
                let mut guard = Span::enter("tagged_operation");
                guard.set_tag("user_id", "123");
                guard.set_tag("operation", "create");
            }
        })
        .await;

        let entries = collector.drain();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            TelemetryEntry::Span(span) => {
                assert_eq!(span.common.tags.get("user_id"), Some(&"123".to_string()));
                assert_eq!(span.common.tags.get("operation"), Some(&"create".to_string()));
            }
            _ => panic!("Expected span entry"),
        }
    }

    #[tokio::test]
    async fn span_outside_context() {
        // Span::enter outside of context should return noop guard
        let guard = Span::enter("orphan_span");
        assert!(!guard.is_active());
        drop(guard); // Should not panic
    }

    #[tokio::test]
    async fn span_parent_chain() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            let _outer = Span::enter("outer");
            let _inner = Span::enter("inner");
        })
        .await;

        let entries = collector.drain();
        assert_eq!(entries.len(), 2);

        // Inner span should have outer span as parent
        let inner = entries.iter().find_map(|e| match e {
            TelemetryEntry::Span(s) if s.name == "inner" => Some(s),
            _ => None,
        }).unwrap();

        let outer = entries.iter().find_map(|e| match e {
            TelemetryEntry::Span(s) if s.name == "outer" => Some(s),
            _ => None,
        }).unwrap();

        // Inner's parent should be outer's span_id
        assert!(inner.parent_span_id.is_some());
        assert_eq!(
            inner.parent_span_id.as_ref().map(|s| s.as_str()),
            outer.common.span_id.as_ref().map(|s| s.as_str())
        );
    }
}
