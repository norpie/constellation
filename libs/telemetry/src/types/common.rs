use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use ulid::Ulid;

/// Microseconds since Unix epoch
pub type Timestamp = u64;

/// 128-bit trace ID in OpenTelemetry format (32 hex chars)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(String);

impl TraceId {
    /// Generate a new random trace ID
    pub fn new() -> Self {
        let bytes: [u8; 16] = rand::random();
        Self(hex::encode(&bytes))
    }

    /// Create from existing hex string
    pub fn from_hex(s: impl Into<String>) -> Option<Self> {
        let s = s.into();
        if s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            Some(Self(s.to_lowercase()))
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 64-bit span ID in OpenTelemetry format (16 hex chars)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanId(String);

impl SpanId {
    /// Generate a new random span ID
    pub fn new() -> Self {
        let bytes: [u8; 8] = rand::random();
        Self(hex::encode(&bytes))
    }

    /// Create from existing hex string
    pub fn from_hex(s: impl Into<String>) -> Option<Self> {
        let s = s.into();
        if s.len() == 16 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            Some(Self(s.to_lowercase()))
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SpanId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SpanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Entry ID using ULID (time-sortable, 26 chars)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntryId(Ulid);

impl EntryId {
    /// Generate a new ULID
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    /// Create from existing ULID
    pub fn from_ulid(ulid: Ulid) -> Self {
        Self(ulid)
    }

    /// Get the underlying ULID
    pub fn as_ulid(&self) -> &Ulid {
        &self.0
    }

    /// Get timestamp in milliseconds from the ULID
    pub fn timestamp_ms(&self) -> u64 {
        self.0.timestamp_ms()
    }

    /// Convert to bytes (128-bit)
    pub fn to_bytes(&self) -> [u8; 16] {
        self.0.to_bytes()
    }

    /// Create from bytes
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Ulid::from_bytes(bytes))
    }
}

impl Default for EntryId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Common fields present on all telemetry entries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommonFields {
    /// Unique entry identifier (ULID, time-sorted)
    pub id: EntryId,

    /// Timestamp in microseconds since Unix epoch
    pub timestamp: Timestamp,

    /// Service name that generated this entry
    pub service: String,

    /// Node/instance identifier
    pub node_id: String,

    /// Trace ID for correlation (optional)
    pub trace_id: Option<TraceId>,

    /// Span ID for correlation (optional)
    pub span_id: Option<SpanId>,

    /// Arbitrary key-value tags
    pub tags: HashMap<String, String>,
}

impl CommonFields {
    /// Create new common fields with current timestamp
    pub fn new(service: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self {
            id: EntryId::new(),
            timestamp: current_timestamp_micros(),
            service: service.into(),
            node_id: node_id.into(),
            trace_id: None,
            span_id: None,
            tags: HashMap::new(),
        }
    }

    /// Set trace ID
    pub fn with_trace_id(mut self, trace_id: TraceId) -> Self {
        self.trace_id = Some(trace_id);
        self
    }

    /// Set span ID
    pub fn with_span_id(mut self, span_id: SpanId) -> Self {
        self.span_id = Some(span_id);
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Add multiple tags
    pub fn with_tags(mut self, tags: HashMap<String, String>) -> Self {
        self.tags.extend(tags);
        self
    }
}

/// Get current timestamp in microseconds since Unix epoch
pub fn current_timestamp_micros() -> Timestamp {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before Unix epoch")
        .as_micros() as u64
}

// We need hex encoding for trace/span IDs
mod hex {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(bytes: &[u8]) -> String {
        let mut result = String::with_capacity(bytes.len() * 2);
        for &byte in bytes {
            result.push(HEX_CHARS[(byte >> 4) as usize] as char);
            result.push(HEX_CHARS[(byte & 0x0f) as usize] as char);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_id_generation() {
        let id = TraceId::new();
        assert_eq!(id.as_str().len(), 32);
        assert!(id.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn trace_id_from_hex() {
        let valid = TraceId::from_hex("0123456789abcdef0123456789abcdef");
        assert!(valid.is_some());

        let invalid_len = TraceId::from_hex("0123456789abcdef");
        assert!(invalid_len.is_none());

        let invalid_chars = TraceId::from_hex("0123456789abcdefghijklmnopqrstuv");
        assert!(invalid_chars.is_none());
    }

    #[test]
    fn span_id_generation() {
        let id = SpanId::new();
        assert_eq!(id.as_str().len(), 16);
        assert!(id.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn entry_id_generation() {
        let id1 = EntryId::new();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = EntryId::new();

        // ULIDs are time-sorted
        assert!(id2.as_ulid() > id1.as_ulid());
    }

    #[test]
    fn common_fields_builder() {
        let fields = CommonFields::new("auth-service", "auth-1")
            .with_trace_id(TraceId::new())
            .with_tag("env", "prod");

        assert_eq!(fields.service, "auth-service");
        assert_eq!(fields.node_id, "auth-1");
        assert!(fields.trace_id.is_some());
        assert_eq!(fields.tags.get("env"), Some(&"prod".to_string()));
    }
}
