//! Context-propagating task spawning.
//!
//! Wrappers around tokio's spawn functions that automatically propagate
//! the `TelemetryContext` to spawned tasks.

use crate::context::TelemetryContext;
use std::future::Future;
use tokio::task::JoinHandle;

/// Spawn a future with the current telemetry context propagated.
///
/// This is a drop-in replacement for `tokio::spawn` that ensures the
/// spawned task inherits the current trace_id, span_id, and other context.
///
/// # Example
///
/// ```text
/// use constellation_telemetry::{spawn, TelemetryContext, info};
///
/// let ctx = TelemetryContext::new("my-service", "node-1");
/// ctx.scope(async {
///     // This spawned task will have the same trace_id
///     spawn(async {
///         info!("Background work"); // Will have correct trace_id
///     });
/// }).await;
/// ```
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    // Capture current context if available
    let ctx = TelemetryContext::try_current();

    tokio::spawn(async move {
        match ctx {
            Some(ctx) => ctx.scope(future).await,
            None => future.await,
        }
    })
}

/// Spawn a future with a specific context.
///
/// Use this when you want to explicitly set the context for the spawned task,
/// rather than inheriting from the current context.
pub fn spawn_with_context<F>(ctx: TelemetryContext, future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(async move { ctx.scope(future).await })
}

/// Spawn a blocking task with the current telemetry context propagated.
///
/// This is a drop-in replacement for `tokio::task::spawn_blocking` that
/// ensures the spawned task inherits the current telemetry context.
///
/// # Example
///
/// ```text
/// use constellation_telemetry::{spawn_blocking, TelemetryContext, info};
///
/// let ctx = TelemetryContext::new("my-service", "node-1");
/// ctx.scope(async {
///     spawn_blocking(|| {
///         // Runs on blocking thread pool with context
///         info!("Blocking work");
///     }).await.unwrap();
/// }).await;
/// ```
pub fn spawn_blocking<F, R>(f: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    // Capture current context if available
    let ctx = TelemetryContext::try_current();

    tokio::task::spawn_blocking(move || match ctx {
        Some(ctx) => ctx.scope_sync(f),
        None => f(),
    })
}

/// Spawn a blocking task with a specific context.
pub fn spawn_blocking_with_context<F, R>(ctx: TelemetryContext, f: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(move || ctx.scope_sync(f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        drain, set_global_collector, BufferCollector, CollectorConfig, TelemetryEntry,
    };
    use std::sync::Arc;

    fn setup_collector() {
        let _ = set_global_collector(Arc::new(BufferCollector::new(CollectorConfig::new(1000))));
        let _ = drain();
    }

    #[tokio::test]
    async fn spawn_propagates_context() {
        setup_collector();

        let ctx = TelemetryContext::new("spawn-test", "node-1");
        let trace_id = ctx.trace_id.clone();

        ctx.scope(async {
            let handle = spawn(async {
                // Should have context inside spawned task
                TelemetryContext::with(|c| c.trace_id.clone())
            });

            let inner_trace_id = handle.await.unwrap();
            assert_eq!(inner_trace_id, Some(trace_id));
        })
        .await;
    }

    #[tokio::test]
    async fn spawn_without_context() {
        // Outside any context
        let handle = spawn(async {
            TelemetryContext::try_current()
        });

        let result = handle.await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn spawn_with_explicit_context() {
        let ctx = TelemetryContext::new("explicit-test", "node-1");
        let trace_id = ctx.trace_id.clone();

        // No outer context
        let handle = spawn_with_context(ctx, async {
            TelemetryContext::with(|c| c.trace_id.clone())
        });

        let inner_trace_id = handle.await.unwrap();
        assert_eq!(inner_trace_id, Some(trace_id));
    }

    #[tokio::test]
    async fn spawn_blocking_propagates_context() {
        setup_collector();

        let ctx = TelemetryContext::new("blocking-test", "node-1");
        let trace_id = ctx.trace_id.clone();

        ctx.scope(async {
            let handle = spawn_blocking(move || {
                TelemetryContext::with(|c| c.trace_id.clone())
            });

            let inner_trace_id = handle.await.unwrap();
            assert_eq!(inner_trace_id, Some(trace_id));
        })
        .await;
    }

    #[tokio::test]
    async fn spawn_blocking_without_context() {
        let handle = spawn_blocking(|| {
            TelemetryContext::try_current()
        });

        let result = handle.await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn nested_spawn_propagation() {
        setup_collector();

        let ctx = TelemetryContext::new("nested-test", "node-1");
        let trace_id = ctx.trace_id.clone();

        ctx.scope(async {
            let handle = spawn(async {
                // First level spawn
                let inner_handle = spawn(async {
                    // Second level spawn - should still have context
                    TelemetryContext::with(|c| c.trace_id.clone())
                });
                inner_handle.await.unwrap()
            });

            let inner_trace_id = handle.await.unwrap();
            assert_eq!(inner_trace_id, Some(trace_id));
        })
        .await;
    }

    #[tokio::test]
    async fn spawn_captures_service_info() {
        setup_collector();

        let ctx = TelemetryContext::new("service-test", "node-42");

        ctx.scope(async {
            let handle = spawn(async {
                TelemetryContext::with(|c| (c.service.clone(), c.node_id.clone()))
            });

            let (service, node_id) = handle.await.unwrap().unwrap();
            assert_eq!(service, "service-test");
            assert_eq!(node_id, "node-42");
        })
        .await;
    }

    #[tokio::test]
    async fn spawn_telemetry_collection() {
        setup_collector();

        let ctx = TelemetryContext::new("collect-test", "node-1");

        ctx.scope(async {
            let handle = spawn(async {
                crate::info!("From spawned task");
            });

            handle.await.unwrap();
        })
        .await;

        let entries: Vec<_> = drain()
            .into_iter()
            .filter(|e| e.service() == "collect-test")
            .collect();

        assert_eq!(entries.len(), 1);
        if let TelemetryEntry::Log(log) = &entries[0] {
            assert_eq!(log.message, "From spawned task");
        }
    }
}
