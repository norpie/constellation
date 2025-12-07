//! Retention and cleanup for telemetry entries.

use crate::config::RetentionConfig;
use crate::error::Result;
use crate::key::{PrimaryKey, PRIMARY_KEY_SIZE};
use crate::store::Datapad;
use constellation_telemetry::{current_timestamp_micros, EntryType, Timestamp};

/// Result of a cleanup operation.
#[derive(Debug, Clone, Default)]
pub struct CleanupResult {
    /// Number of log entries deleted.
    pub logs_deleted: usize,

    /// Number of metric entries deleted.
    pub metrics_deleted: usize,

    /// Number of span entries deleted.
    pub spans_deleted: usize,
}

impl CleanupResult {
    /// Total entries deleted.
    pub fn total(&self) -> usize {
        self.logs_deleted + self.metrics_deleted + self.spans_deleted
    }
}

impl Datapad {
    /// Run cleanup based on retention configuration.
    ///
    /// Deletes entries older than the configured retention period for each type.
    pub fn cleanup(&self, config: &RetentionConfig) -> Result<CleanupResult> {
        let now = current_timestamp_micros();
        let mut result = CleanupResult::default();

        // Calculate cutoff times for each entry type
        let log_cutoff = now.saturating_sub(config.logs.as_micros() as u64);
        let metric_cutoff = now.saturating_sub(config.metrics.as_micros() as u64);
        let span_cutoff = now.saturating_sub(config.spans.as_micros() as u64);

        // Find the latest cutoff to scan up to (we need to check all entries up to this point)
        let latest_cutoff = log_cutoff.max(metric_cutoff).max(span_cutoff);

        // Scan entries and collect keys to delete
        let entries = self.db().open_tree("entries")?;
        let start = [0u8; PRIMARY_KEY_SIZE];
        let end = PrimaryKey::range_end(latest_cutoff);

        let mut to_delete = Vec::new();

        for item in entries.range(start..=end) {
            let (key, _) = item?;
            if key.len() != PRIMARY_KEY_SIZE {
                continue;
            }

            if let Some(pk) = PrimaryKey::decode(&key) {
                let should_delete = match pk.entry_type {
                    EntryType::Log => pk.timestamp < log_cutoff,
                    EntryType::Metric => pk.timestamp < metric_cutoff,
                    EntryType::Span => pk.timestamp < span_cutoff,
                };

                if should_delete {
                    to_delete.push((pk.entry_type, key.to_vec()));
                }
            }
        }

        // Delete collected entries
        for (entry_type, key_bytes) in to_delete {
            // Get entry to clean up indices
            if let Some(entry) = self.get_by_bytes(&key_bytes)? {
                // Delete from primary tree
                entries.remove(&key_bytes)?;

                // Clean up indices
                self.remove_from_all_indices(&entry, &key_bytes)?;

                match entry_type {
                    EntryType::Log => result.logs_deleted += 1,
                    EntryType::Metric => result.metrics_deleted += 1,
                    EntryType::Span => result.spans_deleted += 1,
                }
            }
        }

        Ok(result)
    }

    /// Cleanup entries older than a specific timestamp.
    ///
    /// This is a simpler cleanup that deletes all entries before the given time,
    /// regardless of entry type.
    pub fn cleanup_before(&self, cutoff: Timestamp) -> Result<usize> {
        let entries = self.db().open_tree("entries")?;
        let start = [0u8; PRIMARY_KEY_SIZE];
        let end = PrimaryKey::range_end(cutoff);

        let mut to_delete = Vec::new();

        for item in entries.range(start..end) {
            let (key, _) = item?;
            to_delete.push(key.to_vec());
        }

        let count = to_delete.len();

        for key_bytes in to_delete {
            if let Some(entry) = self.get_by_bytes(&key_bytes)? {
                entries.remove(&key_bytes)?;
                self.remove_from_all_indices(&entry, &key_bytes)?;
            }
        }

        Ok(count)
    }

    /// Internal helper to remove entry from all indices.
    fn remove_from_all_indices(
        &self,
        entry: &constellation_telemetry::TelemetryEntry,
        key_bytes: &[u8],
    ) -> Result<()> {
        use constellation_telemetry::TelemetryEntry;

        let db = self.db();

        // Trace ID index
        if let Some(trace_id) = entry.trace_id() {
            let trace_tree = db.open_tree("idx_trace")?;
            remove_key_from_index(&trace_tree, trace_id.as_str().as_bytes(), key_bytes)?;
        }

        // Service index
        let service_tree = db.open_tree("idx_service")?;
        remove_key_from_index(&service_tree, entry.service().as_bytes(), key_bytes)?;

        // Type-specific indices
        match entry {
            TelemetryEntry::Log(log) => {
                let level_tree = db.open_tree("idx_level")?;
                remove_key_from_index(&level_tree, &[log.level.as_u8()], key_bytes)?;
            }
            TelemetryEntry::Metric(metric) => {
                let metric_tree = db.open_tree("idx_metric")?;
                remove_key_from_index(&metric_tree, metric.name.as_bytes(), key_bytes)?;
            }
            TelemetryEntry::Span(_) => {}
        }

        Ok(())
    }
}

/// Remove a primary key from an index entry using composite key format.
fn remove_key_from_index(tree: &sled::Tree, index_value: &[u8], primary_key: &[u8]) -> Result<()> {
    use crate::store::build_index_key;
    let composite_key = build_index_key(index_value, primary_key);
    tree.remove(composite_key)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use constellation_telemetry::{CommonFields, Level, LogEntry, MetricEntry};
    use std::time::Duration;

    fn make_entry_at_time(timestamp: Timestamp, service: &str) -> constellation_telemetry::TelemetryEntry {
        let mut common = CommonFields::new(service, "node-1");
        common.timestamp = timestamp;
        LogEntry::new(common, Level::Info, "test").into()
    }

    #[test]
    fn cleanup_old_entries() {
        let datapad = Datapad::open_temporary().unwrap();

        let now = current_timestamp_micros();
        let one_day_micros = 24 * 60 * 60 * 1_000_000u64;

        // Insert entries with different ages
        let old_entry = make_entry_at_time(now - 10 * one_day_micros, "auth"); // 10 days old
        let recent_entry = make_entry_at_time(now - one_day_micros, "auth"); // 1 day old
        let new_entry = make_entry_at_time(now, "auth"); // now

        datapad.insert(&old_entry).unwrap();
        datapad.insert(&recent_entry).unwrap();
        datapad.insert(&new_entry).unwrap();

        assert_eq!(datapad.count().unwrap(), 3);

        // Cleanup with 7 day retention
        let config = RetentionConfig {
            logs: Duration::from_secs(7 * 24 * 60 * 60),
            metrics: Duration::from_secs(7 * 24 * 60 * 60),
            spans: Duration::from_secs(7 * 24 * 60 * 60),
            rollups: Duration::from_secs(90 * 24 * 60 * 60),
        };

        let result = datapad.cleanup(&config).unwrap();
        assert_eq!(result.logs_deleted, 1);
        assert_eq!(datapad.count().unwrap(), 2);
    }

    #[test]
    fn cleanup_before_timestamp() {
        let datapad = Datapad::open_temporary().unwrap();

        let now = current_timestamp_micros();
        let hour = 60 * 60 * 1_000_000u64;

        datapad.insert(&make_entry_at_time(now - 3 * hour, "a")).unwrap();
        datapad.insert(&make_entry_at_time(now - 2 * hour, "b")).unwrap();
        datapad.insert(&make_entry_at_time(now - 1 * hour, "c")).unwrap();
        datapad.insert(&make_entry_at_time(now, "d")).unwrap();

        assert_eq!(datapad.count().unwrap(), 4);

        // Delete entries older than 90 minutes
        let cutoff = now - (90 * 60 * 1_000_000);
        let deleted = datapad.cleanup_before(cutoff).unwrap();

        assert_eq!(deleted, 2);
        assert_eq!(datapad.count().unwrap(), 2);
    }

    #[test]
    fn cleanup_clears_indices() {
        let datapad = Datapad::open_temporary().unwrap();

        let now = current_timestamp_micros();
        let hour = 60 * 60 * 1_000_000u64;

        // Insert old entry
        let old_entry = make_entry_at_time(now - 10 * hour, "auth");
        datapad.insert(&old_entry).unwrap();

        // Verify it's in the service index
        let results = datapad.query().service("auth").execute().unwrap();
        assert_eq!(results.len(), 1);

        // Cleanup
        let cutoff = now - 5 * hour;
        datapad.cleanup_before(cutoff).unwrap();

        // Verify index is also cleaned
        let results = datapad.query().service("auth").execute().unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn cleanup_respects_entry_type() {
        let datapad = Datapad::open_temporary().unwrap();

        let now = current_timestamp_micros();
        let day = 24 * 60 * 60 * 1_000_000u64;

        // Insert 5-day old log and metric
        let mut log_common = CommonFields::new("auth", "node-1");
        log_common.timestamp = now - 5 * day;
        let log = LogEntry::new(log_common, Level::Info, "old log");

        let mut metric_common = CommonFields::new("auth", "node-1");
        metric_common.timestamp = now - 5 * day;
        let metric = MetricEntry::counter(metric_common, "old_metric");

        datapad.insert(&log.into()).unwrap();
        datapad.insert(&metric.into()).unwrap();

        assert_eq!(datapad.count().unwrap(), 2);

        // Cleanup with different retention: 3 days for logs, 7 days for metrics
        let config = RetentionConfig {
            logs: Duration::from_secs(3 * 24 * 60 * 60),
            metrics: Duration::from_secs(7 * 24 * 60 * 60),
            spans: Duration::from_secs(7 * 24 * 60 * 60),
            rollups: Duration::from_secs(90 * 24 * 60 * 60),
        };

        let result = datapad.cleanup(&config).unwrap();
        assert_eq!(result.logs_deleted, 1);
        assert_eq!(result.metrics_deleted, 0);
        assert_eq!(datapad.count().unwrap(), 1);
    }
}
