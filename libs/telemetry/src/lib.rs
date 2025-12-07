//! Constellation Telemetry
//!
//! Collection API, types, and context propagation for observability.
//!
//! # Overview
//!
//! This crate provides:
//! - **Types**: `LogEntry`, `MetricEntry`, `SpanEntry`, `TelemetryEntry`
//! - **Context**: Task-local telemetry context with trace/span correlation
//! - **Collector**: Buffered collection with WAL overflow
//! - **Macros**: `info!`, `warn!`, `error!`, `debug!`, `metric!`, `span!`
//! - **Spawn**: Context-propagating task spawning
//!
//! # Example
//!
//! ```ignore
//! use constellation_telemetry::{info, metric, span};
//!
//! #[handler]
//! async fn handle_request(req: Request) -> Response {
//!     info!("Processing request", "path" => req.path);
//!     metric!(counter "requests_total");
//!
//!     let result = span!(child "database_query", {
//!         db.query(&req.id).await
//!     });
//!
//!     Response::ok(result)
//! }
//! ```

pub mod collector;
pub mod context;
#[macro_use]
pub mod macros;
pub mod spawn;
pub mod span;
pub mod types;
pub mod wal;

// Re-export commonly used types at crate root
pub use collector::{
    collect, collect_log, collect_metric, collect_span, drain, global_collector,
    set_global_collector, BufferCollector, Collector, CollectorConfig,
};
pub use context::TelemetryContext;
pub use spawn::{spawn, spawn_blocking, spawn_blocking_with_context, spawn_with_context};
pub use span::{in_span, in_span_sync, Span, SpanGuard};
pub use types::{
    current_timestamp_micros, CommonFields, EntryId, EntryType, Level, LogEntry, MetricEntry,
    MetricType, SpanEntry, SpanId, SpanStatus, TelemetryEntry, Timestamp, TraceId,
};
pub use wal::{Wal, WalConfig, WalError, WalManager, WalReader, WalResult};
