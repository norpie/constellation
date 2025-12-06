//! Live span tracking with automatic context management.
//!
//! This module provides `Span` for tracking in-flight operations and `SpanGuard`
//! for RAII-based span lifecycle management. When a span exits (guard drops),
//! it creates a `SpanEntry` and pops from the context's span stack.

use crate::context::TelemetryContext;
use crate::types::{
    current_timestamp_micros, CommonFields, SpanEntry, SpanId, SpanStatus, Timestamp, TraceId,
};
use std::collections::HashMap;

/// A live span tracking an in-flight operation.
///
/// Create with `Span::new()` for root spans or `Span::child()` for nested spans.
/// Call `enter()` to push onto the context stack and get a guard.
#[derive(Debug)]
pub struct Span {
    id: SpanId,
    parent_id: Option<SpanId>,
    trace_id: TraceId,
    name: String,
    start: Timestamp,
    tags: HashMap<String, String>,
    service: String,
    node_id: String,
}

impl Span {
    /// Start a new span using the current context.
    ///
    /// Returns `None` if no telemetry context is active.
    /// The span inherits trace_id from context and becomes a child of the current span.
    pub fn new(name: impl Into<String>) -> Option<Self> {
        TelemetryContext::with(|ctx| {
            Self {
                id: SpanId::new(),
                parent_id: Some(ctx.span_id.clone()),
                trace_id: ctx.trace_id.clone(),
                name: name.into(),
                start: current_timestamp_micros(),
                tags: HashMap::new(),
                service: ctx.service.clone(),
                node_id: ctx.node_id.clone(),
            }
        })
    }

    /// Alias for `new()` - creates a child span of the current context span.
    pub fn child(name: impl Into<String>) -> Option<Self> {
        Self::new(name)
    }

    /// Create a root span (no parent) with explicit trace_id.
    ///
    /// Use this when starting a new trace, e.g., at request entry points.
    pub fn root(
        name: impl Into<String>,
        trace_id: TraceId,
        service: impl Into<String>,
        node_id: impl Into<String>,
    ) -> Self {
        Self {
            id: SpanId::new(),
            parent_id: None,
            trace_id,
            name: name.into(),
            start: current_timestamp_micros(),
            tags: HashMap::new(),
            service: service.into(),
            node_id: node_id.into(),
        }
    }

    /// Add a tag to the span.
    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Add multiple tags to the span.
    pub fn tags(mut self, tags: impl IntoIterator<Item = (String, String)>) -> Self {
        self.tags.extend(tags);
        self
    }

    /// Get the span ID.
    pub fn id(&self) -> &SpanId {
        &self.id
    }

    /// Get the span name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Enter the span, pushing it onto the context's span stack.
    ///
    /// Returns a `SpanGuard` that will automatically exit the span when dropped.
    pub fn enter(self) -> SpanGuard {
        // Push this span onto the context stack
        TelemetryContext::with_mut(|ctx| {
            ctx.push_span(self.id.clone());
        });

        SpanGuard {
            span: Some(self),
            status: SpanStatus::Ok,
        }
    }
}

/// RAII guard that automatically exits a span when dropped.
///
/// The guard pops the span from the context stack and creates a `SpanEntry`
/// when it goes out of scope.
#[derive(Debug)]
pub struct SpanGuard {
    span: Option<Span>,
    status: SpanStatus,
}

impl SpanGuard {
    /// Mark the span as errored.
    pub fn set_error(&mut self) {
        self.status = SpanStatus::Error;
    }

    /// Set the span status.
    pub fn set_status(&mut self, status: SpanStatus) {
        self.status = status;
    }

    /// Add a tag to the span.
    pub fn tag(&mut self, key: impl Into<String>, value: impl Into<String>) {
        if let Some(ref mut span) = self.span {
            span.tags.insert(key.into(), value.into());
        }
    }

    /// Get the span ID.
    pub fn span_id(&self) -> Option<&SpanId> {
        self.span.as_ref().map(|s| &s.id)
    }

    /// Finish the span and return the resulting `SpanEntry`.
    ///
    /// This consumes the guard without triggering the drop logic.
    pub fn finish(mut self) -> Option<SpanEntry> {
        self.finish_inner()
    }

    /// Internal finish logic, used by both `finish()` and `Drop`.
    fn finish_inner(&mut self) -> Option<SpanEntry> {
        let span = self.span.take()?;
        let end = current_timestamp_micros();

        // Pop from context stack
        TelemetryContext::with_mut(|ctx| {
            ctx.pop_span();
        });

        // Build CommonFields
        let mut common = CommonFields::new(&span.service, &span.node_id);
        common.trace_id = Some(span.trace_id);
        common.span_id = Some(span.id.clone());
        common.tags = span.tags;
        // Use span start as the entry timestamp
        common.timestamp = span.start;

        // Create SpanEntry
        let mut entry = SpanEntry::new(common, span.name, span.start, end);
        entry.status = self.status;
        if let Some(parent_id) = span.parent_id {
            entry = entry.with_parent(parent_id);
        }

        Some(entry)
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        // Finish the span and send to collector
        if let Some(entry) = self.finish_inner() {
            crate::collector::collect_span(entry);
        }
    }
}

/// Execute an async block within a new child span.
///
/// Returns `None` if no context is active, otherwise returns `Some(result)`.
pub async fn in_span<F, Fut, T>(name: impl Into<String>, f: F) -> Option<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let span = Span::new(name)?;
    let _guard = span.enter();
    Some(f().await)
}

/// Execute a sync closure within a new child span.
///
/// Returns `None` if no context is active, otherwise returns `Some(result)`.
pub fn in_span_sync<F, T>(name: impl Into<String>, f: F) -> Option<T>
where
    F: FnOnce() -> T,
{
    let span = Span::new(name)?;
    let _guard = span.enter();
    Some(f())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn span_new_without_context() {
        // No context active, should return None
        assert!(Span::new("test").is_none());
    }

    #[tokio::test]
    async fn span_new_with_context() {
        let ctx = TelemetryContext::new("test-service", "node-1");

        ctx.scope(async {
            let span = Span::new("my-operation");
            assert!(span.is_some());

            let span = span.unwrap();
            assert_eq!(span.name(), "my-operation");
            assert!(span.parent_id.is_some()); // Parent is context's root span
        })
        .await;
    }

    #[tokio::test]
    async fn span_enter_updates_context() {
        let ctx = TelemetryContext::new("test-service", "node-1");

        ctx.scope(async {
            let root_span_id = TelemetryContext::with(|c| c.span_id.clone()).unwrap();

            let span = Span::new("child-op").unwrap();
            let child_span_id = span.id().clone();

            let guard = span.enter();

            // Context should now have child as current span
            let current = TelemetryContext::with(|c| c.span_id.clone()).unwrap();
            assert_eq!(current, child_span_id);

            // Parent should be root
            let parent = TelemetryContext::with(|c| c.parent_span_id().cloned()).unwrap();
            assert_eq!(parent, Some(root_span_id.clone()));

            drop(guard);

            // After drop, context should be back to root
            let current = TelemetryContext::with(|c| c.span_id.clone()).unwrap();
            assert_eq!(current, root_span_id);
        })
        .await;
    }

    #[tokio::test]
    async fn span_guard_finish_returns_entry() {
        let ctx = TelemetryContext::new("test-service", "node-1");

        ctx.scope(async {
            let span = Span::new("my-op").unwrap();
            let guard = span.enter();

            let entry = guard.finish();
            assert!(entry.is_some());

            let entry = entry.unwrap();
            assert_eq!(entry.name, "my-op");
            assert_eq!(entry.common.service, "test-service");
            assert_eq!(entry.status, SpanStatus::Ok);
            assert!(entry.parent_span_id.is_some());
        })
        .await;
    }

    #[tokio::test]
    async fn span_guard_set_error() {
        let ctx = TelemetryContext::new("test-service", "node-1");

        ctx.scope(async {
            let span = Span::new("failing-op").unwrap();
            let mut guard = span.enter();
            guard.set_error();

            let entry = guard.finish().unwrap();
            assert_eq!(entry.status, SpanStatus::Error);
        })
        .await;
    }

    #[tokio::test]
    async fn span_with_tags() {
        let ctx = TelemetryContext::new("test-service", "node-1");

        ctx.scope(async {
            let span = Span::new("tagged-op")
                .unwrap()
                .tag("user_id", "123")
                .tag("endpoint", "/api/users");
            let guard = span.enter();

            let entry = guard.finish().unwrap();
            assert_eq!(entry.common.tags.get("user_id"), Some(&"123".to_string()));
            assert_eq!(
                entry.common.tags.get("endpoint"),
                Some(&"/api/users".to_string())
            );
        })
        .await;
    }

    #[tokio::test]
    async fn nested_spans() {
        let ctx = TelemetryContext::new("test-service", "node-1");

        ctx.scope(async {
            let span1 = Span::new("outer").unwrap();
            let span1_id = span1.id().clone();
            let guard1 = span1.enter();

            let span2 = Span::new("middle").unwrap();
            let span2_id = span2.id().clone();
            assert_eq!(span2.parent_id, Some(span1_id.clone()));
            let guard2 = span2.enter();

            let span3 = Span::new("inner").unwrap();
            assert_eq!(span3.parent_id, Some(span2_id.clone()));
            let guard3 = span3.enter();

            // Depth should be 3 (root + 3 children)
            let depth = TelemetryContext::with(|c| c.span_depth()).unwrap();
            assert_eq!(depth, 3);

            drop(guard3);
            drop(guard2);
            drop(guard1);

            // Back to root depth
            let depth = TelemetryContext::with(|c| c.span_depth()).unwrap();
            assert_eq!(depth, 0);
        })
        .await;
    }

    #[tokio::test]
    async fn in_span_helper() {
        let ctx = TelemetryContext::new("test-service", "node-1");

        let result = ctx
            .scope(async {
                in_span("compute", || async { 42 }).await
            })
            .await;

        assert_eq!(result, Some(42));
    }

    #[tokio::test]
    async fn in_span_sync_helper() {
        let ctx = TelemetryContext::new("test-service", "node-1");

        ctx.scope(async {
            let result = in_span_sync("compute", || 42);
            assert_eq!(result, Some(42));
        })
        .await;
    }

    #[test]
    fn root_span_creation() {
        let trace_id = TraceId::new();
        let span = Span::root("root-op", trace_id.clone(), "my-service", "node-1");

        assert_eq!(span.name(), "root-op");
        assert!(span.parent_id.is_none());
        assert_eq!(span.trace_id, trace_id);
    }

    #[tokio::test]
    async fn span_timing() {
        let ctx = TelemetryContext::new("test-service", "node-1");

        ctx.scope(async {
            let span = Span::new("timed-op").unwrap();
            let start = span.start;

            // Small delay
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            let guard = span.enter();
            let entry = guard.finish().unwrap();

            assert_eq!(entry.start, start);
            assert!(entry.end > entry.start);
            assert!(entry.duration_micros() >= 10_000); // At least 10ms
        })
        .await;
    }
}
