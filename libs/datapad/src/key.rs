//! Primary key encoding for telemetry entries.
//!
//! Keys are designed for efficient time-range scans:
//! `[timestamp: u64][entry_type: u8][ulid: 128-bit]`
//!
//! Using big-endian for timestamp ensures chronological ordering.

use constellation_telemetry::{EntryId, EntryType, Timestamp};

/// Primary key size in bytes: 8 (timestamp) + 1 (type) + 16 (ulid) = 25
pub const PRIMARY_KEY_SIZE: usize = 25;

/// Primary key for telemetry entries.
///
/// Encodes to 25 bytes in a format that sorts by:
/// 1. Timestamp (chronological order)
/// 2. Entry type (logs, metrics, spans grouped)
/// 3. ULID (uniqueness within same timestamp+type)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryKey {
    pub timestamp: Timestamp,
    pub entry_type: EntryType,
    pub id: EntryId,
}

impl PrimaryKey {
    /// Create a new primary key.
    pub fn new(timestamp: Timestamp, entry_type: EntryType, id: EntryId) -> Self {
        Self {
            timestamp,
            entry_type,
            id,
        }
    }

    /// Encode to bytes for storage.
    pub fn encode(&self) -> [u8; PRIMARY_KEY_SIZE] {
        let mut buf = [0u8; PRIMARY_KEY_SIZE];

        // Timestamp: 8 bytes big-endian
        buf[0..8].copy_from_slice(&self.timestamp.to_be_bytes());

        // Entry type: 1 byte
        buf[8] = self.entry_type.as_u8();

        // ULID: 16 bytes
        buf[9..25].copy_from_slice(&self.id.to_bytes());

        buf
    }

    /// Decode from bytes.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != PRIMARY_KEY_SIZE {
            return None;
        }

        let timestamp = u64::from_be_bytes(bytes[0..8].try_into().ok()?);
        let entry_type = EntryType::from_u8(bytes[8])?;

        let mut ulid_bytes = [0u8; 16];
        ulid_bytes.copy_from_slice(&bytes[9..25]);
        let id = EntryId::from_bytes(ulid_bytes);

        Some(Self {
            timestamp,
            entry_type,
            id,
        })
    }

    /// Create a range start key for time-based queries.
    /// Returns the smallest possible key for the given timestamp.
    pub fn range_start(timestamp: Timestamp) -> [u8; PRIMARY_KEY_SIZE] {
        let mut buf = [0u8; PRIMARY_KEY_SIZE];
        buf[0..8].copy_from_slice(&timestamp.to_be_bytes());
        // Rest is zeros (smallest possible values)
        buf
    }

    /// Create a range end key for time-based queries.
    /// Returns one past the largest possible key for the given timestamp.
    pub fn range_end(timestamp: Timestamp) -> [u8; PRIMARY_KEY_SIZE] {
        let mut buf = [0xffu8; PRIMARY_KEY_SIZE];
        buf[0..8].copy_from_slice(&timestamp.to_be_bytes());
        // Rest is 0xff (largest possible values)
        buf
    }

    /// Create a range start key for a specific entry type at a timestamp.
    pub fn range_start_typed(timestamp: Timestamp, entry_type: EntryType) -> [u8; PRIMARY_KEY_SIZE] {
        let mut buf = [0u8; PRIMARY_KEY_SIZE];
        buf[0..8].copy_from_slice(&timestamp.to_be_bytes());
        buf[8] = entry_type.as_u8();
        buf
    }

    /// Create a range end key for a specific entry type at a timestamp.
    pub fn range_end_typed(timestamp: Timestamp, entry_type: EntryType) -> [u8; PRIMARY_KEY_SIZE] {
        let mut buf = [0xffu8; PRIMARY_KEY_SIZE];
        buf[0..8].copy_from_slice(&timestamp.to_be_bytes());
        buf[8] = entry_type.as_u8();
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let id = EntryId::new();
        let key = PrimaryKey::new(1234567890, EntryType::Log, id.clone());

        let encoded = key.encode();
        let decoded = PrimaryKey::decode(&encoded).unwrap();

        assert_eq!(decoded.timestamp, key.timestamp);
        assert_eq!(decoded.entry_type, key.entry_type);
        assert_eq!(decoded.id.to_bytes(), key.id.to_bytes());
    }

    #[test]
    fn keys_sort_chronologically() {
        let id1 = EntryId::new();
        let id2 = EntryId::new();

        let key1 = PrimaryKey::new(1000, EntryType::Log, id1);
        let key2 = PrimaryKey::new(2000, EntryType::Log, id2);

        assert!(key1.encode() < key2.encode());
    }

    #[test]
    fn keys_sort_by_type_within_timestamp() {
        let id1 = EntryId::new();
        let id2 = EntryId::new();
        let id3 = EntryId::new();

        let log_key = PrimaryKey::new(1000, EntryType::Log, id1);
        let metric_key = PrimaryKey::new(1000, EntryType::Metric, id2);
        let span_key = PrimaryKey::new(1000, EntryType::Span, id3);

        assert!(log_key.encode() < metric_key.encode());
        assert!(metric_key.encode() < span_key.encode());
    }

    #[test]
    fn decode_invalid_length() {
        assert!(PrimaryKey::decode(&[0u8; 10]).is_none());
        assert!(PrimaryKey::decode(&[0u8; 30]).is_none());
    }

    #[test]
    fn decode_invalid_entry_type() {
        let mut buf = [0u8; PRIMARY_KEY_SIZE];
        buf[8] = 255; // Invalid entry type
        assert!(PrimaryKey::decode(&buf).is_none());
    }

    #[test]
    fn range_keys() {
        let start = PrimaryKey::range_start(1000);
        let end = PrimaryKey::range_end(1000);

        // Any key at timestamp 1000 should be between start and end
        let id = EntryId::new();
        let key = PrimaryKey::new(1000, EntryType::Metric, id);
        let encoded = key.encode();

        assert!(encoded >= start);
        assert!(encoded <= end);
    }
}
