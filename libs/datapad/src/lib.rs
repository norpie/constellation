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
//! ```text
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

pub mod aggregation;
pub mod config;
pub mod error;
pub mod key;
pub mod query;
pub mod retention;
pub mod store;

// Re-export main types
pub use aggregation::{RollupEntry, RollupInterval};
pub use config::{DatapadConfig, RetentionConfig, StorageMode};
pub use error::{Error, Result};
pub use key::PrimaryKey;
pub use query::{QueryBuilder, QueryFilter};
pub use retention::CleanupResult;
pub use store::Datapad;

// Re-export telemetry types for convenience
pub use constellation_telemetry::types::*;
