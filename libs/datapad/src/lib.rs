//! Constellation Datapad
//!
//! Unified telemetry storage engine built on sled.
//!
//! # Overview
//!
//! Datapad provides:
//! - **Unified storage**: Single database for logs, metrics, and spans
//! - **Time-sorted keys**: Efficient range scans by timestamp
//! - **Secondary indices**: Fast lookups by trace_id, service, metric name, etc.
//! - **Query builder**: Fluent API for building queries
//! - **Retention**: Configurable cleanup policies per entry type
//! - **Aggregation**: Histogram rollups (1min, 1hr, 1day)
//!
//! # Example
//!
//! ```ignore
//! use constellation_datapad::{Datapad, DatapadConfig};
//! use constellation_telemetry::{LogEntry, TelemetryEntry};
//!
//! let config = DatapadConfig::default();
//! let datapad = Datapad::open(&config)?;
//!
//! // Insert entries
//! datapad.insert(entry)?;
//!
//! // Query
//! let results = datapad.query()
//!     .time_range(start, end)
//!     .service("auth")
//!     .execute()?;
//! ```

// Re-export telemetry types for convenience
pub use constellation_telemetry::types::*;

// Future modules (not yet implemented):
// pub mod store;
// pub mod config;
// pub mod key;
// pub mod index;
// pub mod query;
// pub mod retention;
// pub mod aggregation;
