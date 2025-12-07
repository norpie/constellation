//! Core Datapad storage.

use crate::config::{DatapadConfig, StorageMode};
use crate::error::{Error, Result};
use crate::key::PrimaryKey;
use constellation_telemetry::TelemetryEntry;
use sled::Db;

/// Tree names for the database.
mod trees {
    /// Primary entries tree (key -> serialized entry).
    pub const ENTRIES: &str = "entries";

    /// Trace ID index (trace_id -> list of primary keys).
    pub const TRACE_INDEX: &str = "idx_trace";

    /// Service index (service -> list of primary keys).
    pub const SERVICE_INDEX: &str = "idx_service";

    /// Metric name index (metric_name -> list of primary keys).
    pub const METRIC_INDEX: &str = "idx_metric";

    /// Log level index (level -> list of primary keys).
    pub const LEVEL_INDEX: &str = "idx_level";
}

/// Unified telemetry storage engine.
pub struct Datapad {
    db: Db,
    #[allow(dead_code)]
    config: DatapadConfig,
}

impl Datapad {
    /// Open a Datapad with the given configuration.
    pub fn open(config: &DatapadConfig) -> Result<Self> {
        let db = match &config.storage {
            StorageMode::Path(path) => sled::open(path)?,
            StorageMode::Temporary => sled::Config::new().temporary(true).open()?,
        };

        // Ensure all trees exist
        db.open_tree(trees::ENTRIES)?;
        db.open_tree(trees::TRACE_INDEX)?;
        db.open_tree(trees::SERVICE_INDEX)?;
        db.open_tree(trees::METRIC_INDEX)?;
        db.open_tree(trees::LEVEL_INDEX)?;

        Ok(Self {
            db,
            config: config.clone(),
        })
    }

    /// Open a temporary Datapad (for testing).
    pub fn open_temporary() -> Result<Self> {
        Self::open(&DatapadConfig::temporary())
    }

    /// Insert a single telemetry entry.
    pub fn insert(&self, entry: &TelemetryEntry) -> Result<PrimaryKey> {
        let key = PrimaryKey::new(entry.timestamp(), entry.entry_type(), entry.id().clone());
        let key_bytes = key.encode();

        // Serialize entry
        let value = serialize_entry(entry)?;

        // Insert into primary tree
        let entries = self.db.open_tree(trees::ENTRIES)?;
        entries.insert(&key_bytes, value)?;

        // Update indices
        self.update_indices(entry, &key_bytes)?;

        Ok(key)
    }

    /// Insert multiple entries in a batch.
    pub fn insert_batch(&self, entries: &[TelemetryEntry]) -> Result<Vec<PrimaryKey>> {
        let entries_tree = self.db.open_tree(trees::ENTRIES)?;
        let mut keys = Vec::with_capacity(entries.len());

        // Use a batch for atomic insertion
        let mut batch = sled::Batch::default();

        for entry in entries {
            let key = PrimaryKey::new(entry.timestamp(), entry.entry_type(), entry.id().clone());
            let key_bytes = key.encode();
            let value = serialize_entry(entry)?;

            batch.insert(&key_bytes, value);
            keys.push(key);
        }

        entries_tree.apply_batch(batch)?;

        // Update indices for each entry
        for (entry, key) in entries.iter().zip(&keys) {
            self.update_indices(entry, &key.encode())?;
        }

        Ok(keys)
    }

    /// Get an entry by its primary key.
    pub fn get(&self, key: &PrimaryKey) -> Result<Option<TelemetryEntry>> {
        let entries = self.db.open_tree(trees::ENTRIES)?;
        let key_bytes = key.encode();

        match entries.get(&key_bytes)? {
            Some(value) => {
                let entry = deserialize_entry(&value)?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    /// Get an entry by raw key bytes.
    pub fn get_by_bytes(&self, key_bytes: &[u8]) -> Result<Option<TelemetryEntry>> {
        let entries = self.db.open_tree(trees::ENTRIES)?;

        match entries.get(key_bytes)? {
            Some(value) => {
                let entry = deserialize_entry(&value)?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    /// Delete an entry by its primary key.
    pub fn delete(&self, key: &PrimaryKey) -> Result<bool> {
        let entries = self.db.open_tree(trees::ENTRIES)?;
        let key_bytes = key.encode();

        // First get the entry to clean up indices
        if let Some(value) = entries.get(&key_bytes)? {
            let entry = deserialize_entry(&value)?;
            self.remove_from_indices(&entry, &key_bytes)?;
            entries.remove(&key_bytes)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Count entries in the primary tree.
    pub fn count(&self) -> Result<usize> {
        let entries = self.db.open_tree(trees::ENTRIES)?;
        Ok(entries.len())
    }

    /// Flush all pending writes to disk.
    pub fn flush(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }

    /// Update secondary indices for an entry.
    fn update_indices(&self, entry: &TelemetryEntry, key_bytes: &[u8]) -> Result<()> {
        // Trace ID index
        if let Some(trace_id) = entry.trace_id() {
            let trace_tree = self.db.open_tree(trees::TRACE_INDEX)?;
            append_to_index(&trace_tree, trace_id.as_str().as_bytes(), key_bytes)?;
        }

        // Service index
        let service_tree = self.db.open_tree(trees::SERVICE_INDEX)?;
        append_to_index(&service_tree, entry.service().as_bytes(), key_bytes)?;

        // Type-specific indices
        match entry {
            TelemetryEntry::Log(log) => {
                let level_tree = self.db.open_tree(trees::LEVEL_INDEX)?;
                let level_key = [log.level.as_u8()];
                append_to_index(&level_tree, &level_key, key_bytes)?;
            }
            TelemetryEntry::Metric(metric) => {
                let metric_tree = self.db.open_tree(trees::METRIC_INDEX)?;
                append_to_index(&metric_tree, metric.name.as_bytes(), key_bytes)?;
            }
            TelemetryEntry::Span(_) => {
                // Spans don't have additional indices beyond trace_id
            }
        }

        Ok(())
    }

    /// Remove entry from secondary indices.
    fn remove_from_indices(&self, entry: &TelemetryEntry, key_bytes: &[u8]) -> Result<()> {
        // Trace ID index
        if let Some(trace_id) = entry.trace_id() {
            let trace_tree = self.db.open_tree(trees::TRACE_INDEX)?;
            remove_from_index(&trace_tree, trace_id.as_str().as_bytes(), key_bytes)?;
        }

        // Service index
        let service_tree = self.db.open_tree(trees::SERVICE_INDEX)?;
        remove_from_index(&service_tree, entry.service().as_bytes(), key_bytes)?;

        // Type-specific indices
        match entry {
            TelemetryEntry::Log(log) => {
                let level_tree = self.db.open_tree(trees::LEVEL_INDEX)?;
                let level_key = [log.level.as_u8()];
                remove_from_index(&level_tree, &level_key, key_bytes)?;
            }
            TelemetryEntry::Metric(metric) => {
                let metric_tree = self.db.open_tree(trees::METRIC_INDEX)?;
                remove_from_index(&metric_tree, metric.name.as_bytes(), key_bytes)?;
            }
            TelemetryEntry::Span(_) => {}
        }

        Ok(())
    }

    /// Access the underlying sled database (for advanced queries).
    pub fn db(&self) -> &Db {
        &self.db
    }
}

/// Serialize a telemetry entry to bytes.
fn serialize_entry(entry: &TelemetryEntry) -> Result<Vec<u8>> {
    serde_json::to_vec(entry).map_err(|e| Error::Serialization(e.to_string()))
}

/// Deserialize a telemetry entry from bytes.
fn deserialize_entry(bytes: &[u8]) -> Result<TelemetryEntry> {
    serde_json::from_slice(bytes).map_err(|e| Error::Serialization(e.to_string()))
}

/// Separator byte between index value and primary key in composite index keys.
pub(crate) const INDEX_SEPARATOR: u8 = 0x00;

/// Build a composite index key: {index_value}{separator}{primary_key}
pub(crate) fn build_index_key(index_value: &[u8], primary_key: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(index_value.len() + 1 + primary_key.len());
    key.extend_from_slice(index_value);
    key.push(INDEX_SEPARATOR);
    key.extend_from_slice(primary_key);
    key
}

/// Build the start of a range scan for an index value.
pub(crate) fn index_range_start(index_value: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(index_value.len() + 1);
    key.extend_from_slice(index_value);
    key.push(INDEX_SEPARATOR);
    key
}

/// Build the end of a range scan for an index value (exclusive).
pub(crate) fn index_range_end(index_value: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(index_value.len() + 1);
    key.extend_from_slice(index_value);
    key.push(INDEX_SEPARATOR + 1); // One past the separator
    key
}

/// Add a primary key to an index using composite key strategy.
/// Key format: {index_value}\x00{primary_key} -> empty value
fn append_to_index(tree: &sled::Tree, index_value: &[u8], primary_key: &[u8]) -> Result<()> {
    let composite_key = build_index_key(index_value, primary_key);
    tree.insert(composite_key, &[] as &[u8])?;
    Ok(())
}

/// Remove a primary key from an index.
fn remove_from_index(tree: &sled::Tree, index_value: &[u8], primary_key: &[u8]) -> Result<()> {
    let composite_key = build_index_key(index_value, primary_key);
    tree.remove(composite_key)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use constellation_telemetry::{CommonFields, Level, LogEntry, MetricEntry, SpanEntry, TraceId};

    fn make_log(service: &str, level: Level, message: &str) -> TelemetryEntry {
        let common = CommonFields::new(service, "node-1");
        LogEntry::new(common, level, message).into()
    }

    fn make_metric(service: &str, name: &str, value: f64) -> TelemetryEntry {
        let common = CommonFields::new(service, "node-1");
        MetricEntry::gauge(common, name, value).into()
    }

    fn make_span(service: &str, name: &str) -> TelemetryEntry {
        let common = CommonFields::new(service, "node-1").with_trace_id(TraceId::new());
        SpanEntry::new(common, name, 1000, 2000).into()
    }

    #[test]
    fn open_temporary() {
        let datapad = Datapad::open_temporary().unwrap();
        assert_eq!(datapad.count().unwrap(), 0);
    }

    #[test]
    fn insert_and_get() {
        let datapad = Datapad::open_temporary().unwrap();

        let log = make_log("auth", Level::Info, "user logged in");
        let key = datapad.insert(&log).unwrap();

        let retrieved = datapad.get(&key).unwrap().unwrap();
        assert_eq!(retrieved.service(), "auth");
    }

    #[test]
    fn insert_batch() {
        let datapad = Datapad::open_temporary().unwrap();

        let entries = vec![
            make_log("auth", Level::Info, "message 1"),
            make_log("auth", Level::Warn, "message 2"),
            make_metric("api", "requests", 100.0),
        ];

        let keys = datapad.insert_batch(&entries).unwrap();
        assert_eq!(keys.len(), 3);
        assert_eq!(datapad.count().unwrap(), 3);
    }

    #[test]
    fn delete_entry() {
        let datapad = Datapad::open_temporary().unwrap();

        let log = make_log("auth", Level::Info, "to be deleted");
        let key = datapad.insert(&log).unwrap();

        assert!(datapad.delete(&key).unwrap());
        assert!(datapad.get(&key).unwrap().is_none());
        assert_eq!(datapad.count().unwrap(), 0);
    }

    #[test]
    fn delete_nonexistent() {
        let datapad = Datapad::open_temporary().unwrap();

        let log = make_log("auth", Level::Info, "test");
        let key = PrimaryKey::new(log.timestamp(), log.entry_type(), log.id().clone());

        assert!(!datapad.delete(&key).unwrap());
    }

    #[test]
    fn span_with_trace_id() {
        let datapad = Datapad::open_temporary().unwrap();

        let span = make_span("api", "handle_request");
        let key = datapad.insert(&span).unwrap();

        let retrieved = datapad.get(&key).unwrap().unwrap();
        assert!(retrieved.trace_id().is_some());
    }
}
