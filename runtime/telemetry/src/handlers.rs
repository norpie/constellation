//! Telemetry service handlers
//!
//! Handlers for ingesting, querying, and managing telemetry data.

use constellation_datapad::Datapad;
use constellation_node::{handler, Data};
use constellation_telemetry::{EntryType, Level, TelemetryEntry, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::Error;

// ============================================================================
// Ingest Handler
// ============================================================================

/// Request to ingest telemetry entries
#[derive(Debug, Deserialize, JsonSchema)]
pub struct IngestRequest {
    /// Batch of telemetry entries to ingest
    pub entries: Vec<TelemetryEntry>,
}

/// Response from ingesting telemetry entries
#[derive(Debug, Serialize, JsonSchema)]
pub struct IngestResponse {
    /// Number of entries successfully ingested
    pub ingested: usize,
}

/// Ingest a batch of telemetry entries into storage
#[handler(route = "_telemetry.ingest")]
pub async fn ingest(
    req: IngestRequest,
    datapad: Data<Datapad>,
) -> Result<IngestResponse, constellation_node::error::Error> {
    let count = req.entries.len();

    datapad
        .insert_batch(&req.entries)
        .map_err(|e| Error::from(e))?;

    Ok(IngestResponse { ingested: count })
}

// ============================================================================
// Query Handler
// ============================================================================

/// Request to query telemetry entries
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct QueryRequest {
    /// Start of time range (inclusive, microseconds since epoch)
    pub start_time: Option<Timestamp>,

    /// End of time range (inclusive, microseconds since epoch)
    pub end_time: Option<Timestamp>,

    /// Filter by entry type (Log, Metric, Span)
    pub entry_type: Option<EntryType>,

    /// Filter by service name
    pub service: Option<String>,

    /// Filter by trace ID
    pub trace_id: Option<String>,

    /// Filter by metric name (only for metrics)
    pub metric_name: Option<String>,

    /// Filter by log level (only for logs)
    pub level: Option<Level>,

    /// Filter by tag key-value pairs
    #[serde(default)]
    pub tags: Vec<(String, String)>,

    /// Maximum number of results
    pub limit: Option<usize>,
}

/// Response from querying telemetry entries
#[derive(Debug, Serialize, JsonSchema)]
pub struct QueryResponse {
    /// Matching telemetry entries
    pub entries: Vec<TelemetryEntry>,
}

/// Query telemetry entries from storage
#[handler(route = "_telemetry.query")]
pub async fn query(
    req: QueryRequest,
    datapad: Data<Datapad>,
) -> Result<QueryResponse, constellation_node::error::Error> {
    let mut builder = datapad.query();

    if let Some(start) = req.start_time {
        builder = builder.start_time(start);
    }
    if let Some(end) = req.end_time {
        builder = builder.end_time(end);
    }
    if let Some(entry_type) = req.entry_type {
        builder = builder.entry_type(entry_type);
    }
    if let Some(ref service) = req.service {
        builder = builder.service(service);
    }
    if let Some(ref trace_id) = req.trace_id {
        builder = builder.trace_id(trace_id);
    }
    if let Some(ref metric_name) = req.metric_name {
        builder = builder.metric_name(metric_name);
    }
    if let Some(level) = req.level {
        builder = builder.level(level);
    }
    for (key, value) in &req.tags {
        builder = builder.tag(key, value);
    }
    if let Some(limit) = req.limit {
        builder = builder.limit(limit);
    }

    let entries = builder.execute().map_err(|e| Error::from(e))?;

    Ok(QueryResponse { entries })
}

// ============================================================================
// Alerts Handler (TODO)
// ============================================================================

// TODO: Implement _telemetry.alerts handler
//
// This handler will list active alerts based on configured alert rules.
// Deferred until the notifications service is implemented.
//
// Planned interface:
// - `_telemetry.alerts` - List active alerts with optional filters
// - AlertRule configuration via separate config/management handlers
