//! Constellation Telemetry
//!
//! Types and collector implementations for observability.
//!
//! # Overview
//!
//! This crate provides:
//! - **Types**: `LogEntry`, `MetricEntry`, `SpanEntry`, `TelemetryEntry`
//! - **Collector**: Buffered collection with WAL overflow
//!
//! Context management, macros, and integration are provided by `constellation-node`.

pub mod collector;
pub mod types;
pub mod wal;

// Re-export commonly used types at crate root
pub use collector::{BufferCollector, Collector, CollectorConfig};
pub use types::{
    current_timestamp_micros, CommonFields, EntryId, EntryType, Level, LogEntry, MetricEntry,
    MetricType, SpanEntry, SpanId, SpanStatus, TelemetryEntry, Timestamp, TraceId,
};
pub use wal::{Wal, WalConfig, WalError, WalManager, WalReader, WalResult};
