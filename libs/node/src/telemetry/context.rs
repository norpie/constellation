//! Telemetry context for trace propagation.
//!
//! The context is stored in task-local storage and provides trace/span IDs
//! for correlation across async operations.

use crate::Data;
use constellation_telemetry::{BufferCollector, SpanId, TraceId};
use std::cell::RefCell;
use std::future::Future;

tokio::task_local! {
    static TELEMETRY_CONTEXT: RefCell<TelemetryContext>;
}

/// Telemetry context for trace propagation.
///
/// Contains trace and span identifiers for correlating telemetry entries
/// across a request or operation. The context is stored in task-local
/// storage and can be accessed from anywhere in the async call stack.
#[derive(Clone)]
pub struct TelemetryContext {
    /// Service name (e.g., "AuthService")
    pub service: String,

    /// Node instance ID (e.g., "auth-1")
    pub node_id: String,

    /// Current trace ID (shared across all spans in a trace)
    pub trace_id: TraceId,

    /// Current span ID (changes for each span)
    pub span_id: SpanId,

    /// Parent span ID (for creating child spans)
    pub parent_span_id: Option<SpanId>,

    /// Reference to the collector for emitting entries
    pub collector: Data<BufferCollector>,

    /// Stack of active span IDs for nested spans
    span_stack: Vec<SpanId>,
}

impl TelemetryContext {
    /// Create a new telemetry context with a fresh trace.
    ///
    /// Use this for framework tasks that need their own trace.
    pub fn new(
        service: impl Into<String>,
        node_id: impl Into<String>,
        collector: Data<BufferCollector>,
    ) -> Self {
        let span_id = SpanId::new();
        Self {
            service: service.into(),
            node_id: node_id.into(),
            trace_id: TraceId::new(),
            span_id: span_id.clone(),
            parent_span_id: None,
            collector,
            span_stack: vec![span_id],
        }
    }

    /// Create a context from an incoming request with existing trace.
    ///
    /// Use this for handlers that receive trace context from RPC headers.
    pub fn from_request(
        service: impl Into<String>,
        node_id: impl Into<String>,
        trace_id: TraceId,
        parent_span_id: Option<SpanId>,
        collector: Data<BufferCollector>,
    ) -> Self {
        let span_id = SpanId::new();
        Self {
            service: service.into(),
            node_id: node_id.into(),
            trace_id,
            span_id: span_id.clone(),
            parent_span_id,
            collector,
            span_stack: vec![span_id],
        }
    }

    /// Run a future with this context in task-local storage.
    ///
    /// The context will be available via `TelemetryContext::current()` and
    /// `TelemetryContext::with()` for the duration of the future.
    pub async fn scope<F, T>(self, f: F) -> T
    where
        F: Future<Output = T>,
    {
        TELEMETRY_CONTEXT
            .scope(RefCell::new(self), f)
            .await
    }

    /// Try to get the current context from task-local storage.
    ///
    /// Returns `None` if no context is set (e.g., outside of a `scope()`).
    pub fn try_current() -> Option<TelemetryContext> {
        TELEMETRY_CONTEXT
            .try_with(|ctx| ctx.borrow().clone())
            .ok()
    }

    /// Get the current context, panicking if none is set.
    ///
    /// # Panics
    ///
    /// Panics if called outside of a `TelemetryContext::scope()`.
    pub fn current() -> TelemetryContext {
        Self::try_current().expect("TelemetryContext::current() called outside of scope")
    }

    /// Access the current context in a closure.
    ///
    /// Returns `None` if no context is set.
    pub fn with<F, R>(f: F) -> Option<R>
    where
        F: FnOnce(&TelemetryContext) -> R,
    {
        TELEMETRY_CONTEXT
            .try_with(|ctx| f(&ctx.borrow()))
            .ok()
    }

    /// Mutably access the current context in a closure.
    ///
    /// Returns `None` if no context is set.
    pub fn with_mut<F, R>(f: F) -> Option<R>
    where
        F: FnOnce(&mut TelemetryContext) -> R,
    {
        TELEMETRY_CONTEXT
            .try_with(|ctx| f(&mut ctx.borrow_mut()))
            .ok()
    }

    /// Push a new span onto the stack (entering a child span).
    ///
    /// Returns the new span ID.
    pub fn push_span(&mut self) -> SpanId {
        let new_span = SpanId::new();
        self.parent_span_id = Some(self.span_id.clone());
        self.span_id = new_span.clone();
        self.span_stack.push(new_span.clone());
        new_span
    }

    /// Pop a span from the stack (exiting a child span).
    ///
    /// Returns the popped span ID, or `None` if at root.
    pub fn pop_span(&mut self) -> Option<SpanId> {
        if self.span_stack.len() <= 1 {
            return None; // Don't pop the root span
        }

        let popped = self.span_stack.pop();

        // Restore parent context
        if let Some(current) = self.span_stack.last() {
            self.span_id = current.clone();
        }

        // Update parent_span_id to grandparent
        self.parent_span_id = if self.span_stack.len() > 1 {
            self.span_stack.get(self.span_stack.len() - 2).cloned()
        } else {
            None
        };

        popped
    }

    /// Get the current span depth (1 = root span).
    pub fn span_depth(&self) -> usize {
        self.span_stack.len()
    }
}

impl std::fmt::Debug for TelemetryContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelemetryContext")
            .field("service", &self.service)
            .field("node_id", &self.node_id)
            .field("trace_id", &self.trace_id)
            .field("span_id", &self.span_id)
            .field("parent_span_id", &self.parent_span_id)
            .field("span_depth", &self.span_stack.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use constellation_telemetry::CollectorConfig;

    fn make_collector() -> Data<BufferCollector> {
        Data::new(BufferCollector::new(CollectorConfig::default()))
    }

    #[tokio::test]
    async fn context_scope_and_current() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector);
        let trace_id = ctx.trace_id.clone();

        ctx.scope(async {
            let current = TelemetryContext::current();
            assert_eq!(current.service, "TestService");
            assert_eq!(current.node_id, "test-1");
            assert_eq!(current.trace_id.as_str(), trace_id.as_str());
        })
        .await;
    }

    #[tokio::test]
    async fn context_with_accessor() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector);

        ctx.scope(async {
            let service = TelemetryContext::with(|ctx| ctx.service.clone());
            assert_eq!(service, Some("TestService".to_string()));
        })
        .await;
    }

    #[tokio::test]
    async fn context_try_current_outside_scope() {
        // Should return None when not in a scope
        assert!(TelemetryContext::try_current().is_none());
    }

    #[tokio::test]
    async fn context_from_request() {
        let collector = make_collector();
        let trace_id = TraceId::new();
        let parent_span = SpanId::new();

        let ctx = TelemetryContext::from_request(
            "TestService",
            "test-1",
            trace_id.clone(),
            Some(parent_span.clone()),
            collector,
        );

        assert_eq!(ctx.trace_id.as_str(), trace_id.as_str());
        assert_eq!(
            ctx.parent_span_id.as_ref().map(|s| s.as_str()),
            Some(parent_span.as_str())
        );
    }

    #[tokio::test]
    async fn span_stack_push_pop() {
        let collector = make_collector();
        let mut ctx = TelemetryContext::new("TestService", "test-1", collector);

        assert_eq!(ctx.span_depth(), 1);
        let root_span = ctx.span_id.clone();

        // Push child span
        let child_span = ctx.push_span();
        assert_eq!(ctx.span_depth(), 2);
        assert_eq!(ctx.span_id.as_str(), child_span.as_str());
        assert_eq!(
            ctx.parent_span_id.as_ref().map(|s| s.as_str()),
            Some(root_span.as_str())
        );

        // Push grandchild span
        let grandchild_span = ctx.push_span();
        assert_eq!(ctx.span_depth(), 3);
        assert_eq!(ctx.span_id.as_str(), grandchild_span.as_str());
        assert_eq!(
            ctx.parent_span_id.as_ref().map(|s| s.as_str()),
            Some(child_span.as_str())
        );

        // Pop grandchild
        let popped = ctx.pop_span();
        assert_eq!(popped.as_ref().map(|s| s.as_str()), Some(grandchild_span.as_str()));
        assert_eq!(ctx.span_depth(), 2);
        assert_eq!(ctx.span_id.as_str(), child_span.as_str());

        // Pop child
        let popped = ctx.pop_span();
        assert_eq!(popped.as_ref().map(|s| s.as_str()), Some(child_span.as_str()));
        assert_eq!(ctx.span_depth(), 1);
        assert_eq!(ctx.span_id.as_str(), root_span.as_str());

        // Can't pop root
        let popped = ctx.pop_span();
        assert!(popped.is_none());
        assert_eq!(ctx.span_depth(), 1);
    }

    #[tokio::test]
    async fn context_with_mut() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector);

        ctx.scope(async {
            // Push a span via with_mut
            let new_span = TelemetryContext::with_mut(|ctx| ctx.push_span());
            assert!(new_span.is_some());

            // Verify depth changed
            let depth = TelemetryContext::with(|ctx| ctx.span_depth());
            assert_eq!(depth, Some(2));
        })
        .await;
    }
}
