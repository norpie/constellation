mod common;
mod log;
mod metric;
mod span;

pub use common::{current_timestamp_micros, CommonFields, EntryId, SpanId, Timestamp, TraceId};
pub use log::{Level, LogEntry};
pub use metric::{MetricEntry, MetricType};
pub use span::{SpanEntry, SpanStatus};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Entry type discriminator for storage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[repr(u8)]
pub enum EntryType {
    Log = 0,
    Metric = 1,
    Span = 2,
}

impl EntryType {
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }

    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Log),
            1 => Some(Self::Metric),
            2 => Some(Self::Span),
            _ => None,
        }
    }
}

/// Unified telemetry entry enum
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum TelemetryEntry {
    Log(LogEntry),
    Metric(MetricEntry),
    Span(SpanEntry),
}

impl TelemetryEntry {
    /// Get the entry type
    pub fn entry_type(&self) -> EntryType {
        match self {
            TelemetryEntry::Log(_) => EntryType::Log,
            TelemetryEntry::Metric(_) => EntryType::Metric,
            TelemetryEntry::Span(_) => EntryType::Span,
        }
    }

    /// Get common fields reference
    pub fn common(&self) -> &CommonFields {
        match self {
            TelemetryEntry::Log(e) => &e.common,
            TelemetryEntry::Metric(e) => &e.common,
            TelemetryEntry::Span(e) => &e.common,
        }
    }

    /// Get mutable common fields reference
    pub fn common_mut(&mut self) -> &mut CommonFields {
        match self {
            TelemetryEntry::Log(e) => &mut e.common,
            TelemetryEntry::Metric(e) => &mut e.common,
            TelemetryEntry::Span(e) => &mut e.common,
        }
    }

    /// Get the entry ID
    pub fn id(&self) -> &EntryId {
        &self.common().id
    }

    /// Get the timestamp
    pub fn timestamp(&self) -> Timestamp {
        self.common().timestamp
    }

    /// Get the service name
    pub fn service(&self) -> &str {
        &self.common().service
    }

    /// Get the node ID
    pub fn node_id(&self) -> &str {
        &self.common().node_id
    }

    /// Get the trace ID if present
    pub fn trace_id(&self) -> Option<&TraceId> {
        self.common().trace_id.as_ref()
    }
}

impl From<LogEntry> for TelemetryEntry {
    fn from(entry: LogEntry) -> Self {
        TelemetryEntry::Log(entry)
    }
}

impl From<MetricEntry> for TelemetryEntry {
    fn from(entry: MetricEntry) -> Self {
        TelemetryEntry::Metric(entry)
    }
}

impl From<SpanEntry> for TelemetryEntry {
    fn from(entry: SpanEntry) -> Self {
        TelemetryEntry::Span(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_type_conversion() {
        assert_eq!(EntryType::Log.as_u8(), 0);
        assert_eq!(EntryType::Metric.as_u8(), 1);
        assert_eq!(EntryType::Span.as_u8(), 2);

        assert_eq!(EntryType::from_u8(0), Some(EntryType::Log));
        assert_eq!(EntryType::from_u8(1), Some(EntryType::Metric));
        assert_eq!(EntryType::from_u8(2), Some(EntryType::Span));
        assert_eq!(EntryType::from_u8(255), None);
    }

    #[test]
    fn telemetry_entry_from_log() {
        let common = CommonFields::new("auth", "auth-1");
        let log = LogEntry::new(common, Level::Info, "test message");
        let entry: TelemetryEntry = log.into();

        assert_eq!(entry.entry_type(), EntryType::Log);
        assert_eq!(entry.service(), "auth");
    }

    #[test]
    fn telemetry_entry_from_metric() {
        let common = CommonFields::new("api", "api-1");
        let metric = MetricEntry::counter(common, "requests_total");
        let entry: TelemetryEntry = metric.into();

        assert_eq!(entry.entry_type(), EntryType::Metric);
    }

    #[test]
    fn telemetry_entry_from_span() {
        let common = CommonFields::new("api", "api-1");
        let span = SpanEntry::new(common, "operation", 1000, 2000);
        let entry: TelemetryEntry = span.into();

        assert_eq!(entry.entry_type(), EntryType::Span);
    }

    #[test]
    fn telemetry_entry_serde() {
        let common = CommonFields::new("auth", "auth-1");
        let log = LogEntry::new(common, Level::Info, "test message");
        let entry: TelemetryEntry = log.into();

        // Serialize to JSON (externally tagged: {"Log": {...}})
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"Log\""));

        // Deserialize back
        let deserialized: TelemetryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.entry_type(), EntryType::Log);
    }

    #[test]
    fn telemetry_entry_bincode() {
        let common = CommonFields::new("auth", "auth-1");
        let log = LogEntry::new(common, Level::Info, "test message");
        let entry: TelemetryEntry = log.into();

        // Bincode round-trip
        let bytes = bincode::serde::encode_to_vec(&entry, bincode::config::standard()).unwrap();
        let (deserialized, _): (TelemetryEntry, _) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(deserialized.entry_type(), EntryType::Log);
        assert_eq!(deserialized.service(), "auth");
    }
}
