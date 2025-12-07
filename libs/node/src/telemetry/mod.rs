//! Telemetry integration for the node framework.
//!
//! This module provides context management, span tracking, and logging macros
//! that integrate with the `constellation-telemetry` collector.
//!
//! # Context Propagation
//!
//! The `TelemetryContext` is stored in task-local storage and automatically
//! propagates through async code. Handlers receive context from incoming RPC
//! requests, while framework tasks create fresh traces.
//!
//! # Usage
//!
//! ```ignore
//! use constellation_node::telemetry::{info, span};
//!
//! // Log with automatic context
//! info!("Processing request");
//!
//! // Create a child span
//! span!("database_query", {
//!     // ... do work ...
//! });
//! ```

mod context;
mod handlers;
mod macros;
mod span;

pub use context::TelemetryContext;
pub use span::{Span, SpanGuard};

// Re-export telemetry types for convenience
pub use constellation_telemetry::{
    current_timestamp_micros, BufferCollector, Collector, CollectorConfig, CommonFields, Level,
    LogEntry, MetricEntry, MetricType, SpanEntry, SpanId, SpanStatus, TelemetryEntry, TraceId,
};
