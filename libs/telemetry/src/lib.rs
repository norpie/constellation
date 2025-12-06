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

pub mod context;
pub mod span;
pub mod types;

// Re-export commonly used types at crate root
pub use context::TelemetryContext;
pub use span::{in_span, in_span_sync, Span, SpanGuard};
pub use types::{
    current_timestamp_micros, CommonFields, EntryId, EntryType, Level, LogEntry, MetricEntry,
    MetricType, SpanEntry, SpanId, SpanStatus, TelemetryEntry, Timestamp, TraceId,
};

// Future modules (not yet implemented):
// pub mod collector;
// pub mod wal;
// pub mod spawn;
// pub mod macros;
