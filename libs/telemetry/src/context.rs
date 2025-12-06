//! Task-local telemetry context for trace correlation.
//!
//! Provides `TelemetryContext` which holds trace/span IDs and propagates
//! through async tasks using tokio's task-local storage.

use crate::types::{SpanId, TraceId};
use std::cell::RefCell;
use std::future::Future;

tokio::task_local! {
    static CONTEXT: RefCell<TelemetryContext>;
}

/// Telemetry context for correlating entries within a trace.
///
/// Stored in task-local storage and automatically propagates trace/span IDs
/// to all telemetry entries created within the context's scope.
#[derive(Debug, Clone)]
pub struct TelemetryContext {
    /// Trace ID for correlating related entries across services
    pub trace_id: TraceId,

    /// Current span ID
    pub span_id: SpanId,

    /// Service name
    pub service: String,

    /// Node/instance identifier
    pub node_id: String,

    /// Stack of parent span IDs for nested spans
    span_stack: Vec<SpanId>,
}

impl TelemetryContext {
    /// Create a new telemetry context with fresh trace and span IDs.
    pub fn new(service: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self {
            trace_id: TraceId::new(),
            span_id: SpanId::new(),
            service: service.into(),
            node_id: node_id.into(),
            span_stack: Vec::new(),
        }
    }

    /// Create a context with an existing trace ID (e.g., from incoming request).
    pub fn with_trace_id(
        trace_id: TraceId,
        service: impl Into<String>,
        node_id: impl Into<String>,
    ) -> Self {
        Self {
            trace_id,
            span_id: SpanId::new(),
            service: service.into(),
            node_id: node_id.into(),
            span_stack: Vec::new(),
        }
    }

    /// Get the current context from task-local storage.
    ///
    /// # Panics
    ///
    /// Panics if called outside of a context scope.
    pub fn current() -> Self {
        Self::try_current().expect("TelemetryContext::current() called outside of context scope")
    }

    /// Try to get the current context from task-local storage.
    ///
    /// Returns `None` if not within a context scope.
    pub fn try_current() -> Option<Self> {
        CONTEXT
            .try_with(|ctx| ctx.borrow().clone())
            .ok()
    }

    /// Run a future within this context's scope.
    ///
    /// The context will be available via `TelemetryContext::current()` for the
    /// duration of the future's execution.
    pub async fn scope<F, T>(self, f: F) -> T
    where
        F: Future<Output = T>,
    {
        CONTEXT.scope(RefCell::new(self), f).await
    }

    /// Run a synchronous closure within this context's scope.
    pub fn scope_sync<F, T>(self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        CONTEXT.sync_scope(RefCell::new(self), f)
    }

    /// Access the current context immutably.
    ///
    /// Returns `None` if not within a context scope.
    pub fn with<F, R>(f: F) -> Option<R>
    where
        F: FnOnce(&TelemetryContext) -> R,
    {
        CONTEXT.try_with(|ctx| f(&ctx.borrow())).ok()
    }

    /// Access the current context mutably.
    ///
    /// Returns `None` if not within a context scope.
    pub fn with_mut<F, R>(f: F) -> Option<R>
    where
        F: FnOnce(&mut TelemetryContext) -> R,
    {
        CONTEXT.try_with(|ctx| f(&mut ctx.borrow_mut())).ok()
    }

    /// Push a new span onto the stack and update current span ID.
    ///
    /// Returns the previous span ID (now parent).
    pub fn push_span(&mut self, new_span_id: SpanId) -> SpanId {
        let parent = std::mem::replace(&mut self.span_id, new_span_id);
        self.span_stack.push(parent.clone());
        parent
    }

    /// Pop the current span and restore the parent.
    ///
    /// Returns the popped span ID, or `None` if at root.
    pub fn pop_span(&mut self) -> Option<SpanId> {
        self.span_stack.pop().map(|parent| {
            std::mem::replace(&mut self.span_id, parent)
        })
    }

    /// Get the current parent span ID (top of stack), if any.
    pub fn parent_span_id(&self) -> Option<&SpanId> {
        self.span_stack.last()
    }

    /// Get the current span stack depth.
    pub fn span_depth(&self) -> usize {
        self.span_stack.len()
    }

    /// Check if we're within a context scope.
    pub fn is_active() -> bool {
        CONTEXT.try_with(|_| ()).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn context_scope_basic() {
        let ctx = TelemetryContext::new("test-service", "node-1");
        let trace_id = ctx.trace_id.clone();

        let result = ctx
            .scope(async {
                let current = TelemetryContext::current();
                assert_eq!(current.trace_id, trace_id);
                assert_eq!(current.service, "test-service");
                assert_eq!(current.node_id, "node-1");
                42
            })
            .await;

        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn context_try_current_outside_scope() {
        assert!(TelemetryContext::try_current().is_none());
        assert!(!TelemetryContext::is_active());
    }

    #[tokio::test]
    async fn context_is_active_in_scope() {
        let ctx = TelemetryContext::new("test", "node");
        ctx.scope(async {
            assert!(TelemetryContext::is_active());
        })
        .await;
    }

    #[tokio::test]
    async fn context_with_access() {
        let ctx = TelemetryContext::new("my-service", "node-1");

        ctx.scope(async {
            let service = TelemetryContext::with(|c| c.service.clone());
            assert_eq!(service, Some("my-service".to_string()));
        })
        .await;
    }

    #[tokio::test]
    async fn context_with_mut_access() {
        let ctx = TelemetryContext::new("my-service", "node-1");

        ctx.scope(async {
            // Mutate the context
            TelemetryContext::with_mut(|c| {
                c.service = "modified-service".to_string();
            });

            // Verify mutation
            let service = TelemetryContext::with(|c| c.service.clone());
            assert_eq!(service, Some("modified-service".to_string()));
        })
        .await;
    }

    #[tokio::test]
    async fn span_stack_push_pop() {
        let ctx = TelemetryContext::new("test", "node");

        ctx.scope(async {
            let root_span = TelemetryContext::with(|c| c.span_id.clone()).unwrap();

            // Push a child span
            let child_span = SpanId::new();
            TelemetryContext::with_mut(|c| {
                c.push_span(child_span.clone());
            });

            // Current span should be child
            let current = TelemetryContext::with(|c| c.span_id.clone()).unwrap();
            assert_eq!(current, child_span);

            // Parent should be root
            let parent = TelemetryContext::with(|c| c.parent_span_id().cloned()).unwrap();
            assert_eq!(parent, Some(root_span.clone()));

            // Depth should be 1
            let depth = TelemetryContext::with(|c| c.span_depth()).unwrap();
            assert_eq!(depth, 1);

            // Pop back to root
            TelemetryContext::with_mut(|c| c.pop_span());

            let current = TelemetryContext::with(|c| c.span_id.clone()).unwrap();
            assert_eq!(current, root_span);

            let depth = TelemetryContext::with(|c| c.span_depth()).unwrap();
            assert_eq!(depth, 0);
        })
        .await;
    }

    #[tokio::test]
    async fn nested_spans() {
        let ctx = TelemetryContext::new("test", "node");

        ctx.scope(async {
            let span1 = SpanId::new();
            let span2 = SpanId::new();
            let span3 = SpanId::new();

            TelemetryContext::with_mut(|c| {
                c.push_span(span1.clone());
                c.push_span(span2.clone());
                c.push_span(span3.clone());
            });

            assert_eq!(TelemetryContext::with(|c| c.span_depth()).unwrap(), 3);

            // Pop all
            TelemetryContext::with_mut(|c| {
                assert_eq!(c.pop_span(), Some(span3));
                assert_eq!(c.pop_span(), Some(span2));
                assert_eq!(c.pop_span(), Some(span1));
                assert_eq!(c.pop_span(), None); // Already at root
            });

            assert_eq!(TelemetryContext::with(|c| c.span_depth()).unwrap(), 0);
        })
        .await;
    }

    #[tokio::test]
    async fn with_existing_trace_id() {
        let trace_id = TraceId::from_hex("0123456789abcdef0123456789abcdef").unwrap();
        let ctx = TelemetryContext::with_trace_id(trace_id.clone(), "service", "node");

        ctx.scope(async {
            let current_trace = TelemetryContext::with(|c| c.trace_id.clone()).unwrap();
            assert_eq!(current_trace, trace_id);
        })
        .await;
    }

    #[test]
    fn sync_scope() {
        let ctx = TelemetryContext::new("sync-test", "node");

        let result = ctx.scope_sync(|| {
            let service = TelemetryContext::with(|c| c.service.clone());
            assert_eq!(service, Some("sync-test".to_string()));
            123
        });

        assert_eq!(result, 123);
    }
}
