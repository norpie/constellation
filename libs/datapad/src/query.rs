//! Query builder for telemetry entries.

use crate::error::Result;
use crate::key::{PrimaryKey, PRIMARY_KEY_SIZE};
use crate::store::Datapad;
use constellation_telemetry::{EntryType, Level, TelemetryEntry, Timestamp};

/// Filter criteria for queries.
#[derive(Debug, Clone, Default)]
pub struct QueryFilter {
    /// Start of time range (inclusive).
    pub start_time: Option<Timestamp>,

    /// End of time range (inclusive).
    pub end_time: Option<Timestamp>,

    /// Filter by entry type.
    pub entry_type: Option<EntryType>,

    /// Filter by service name.
    pub service: Option<String>,

    /// Filter by trace ID.
    pub trace_id: Option<String>,

    /// Filter by metric name (only for metrics).
    pub metric_name: Option<String>,

    /// Filter by log level (only for logs).
    pub level: Option<Level>,

    /// Filter by tag key-value pairs.
    pub tags: Vec<(String, String)>,

    /// Maximum number of results.
    pub limit: Option<usize>,
}

/// Builder for constructing queries.
pub struct QueryBuilder<'a> {
    datapad: &'a Datapad,
    filter: QueryFilter,
}

impl<'a> QueryBuilder<'a> {
    /// Create a new query builder.
    pub fn new(datapad: &'a Datapad) -> Self {
        Self {
            datapad,
            filter: QueryFilter::default(),
        }
    }

    /// Set the time range for the query.
    pub fn time_range(mut self, start: Timestamp, end: Timestamp) -> Self {
        self.filter.start_time = Some(start);
        self.filter.end_time = Some(end);
        self
    }

    /// Set the start time for the query.
    pub fn start_time(mut self, start: Timestamp) -> Self {
        self.filter.start_time = Some(start);
        self
    }

    /// Set the end time for the query.
    pub fn end_time(mut self, end: Timestamp) -> Self {
        self.filter.end_time = Some(end);
        self
    }

    /// Filter by entry type.
    pub fn entry_type(mut self, entry_type: EntryType) -> Self {
        self.filter.entry_type = Some(entry_type);
        self
    }

    /// Filter to only logs.
    pub fn logs(self) -> Self {
        self.entry_type(EntryType::Log)
    }

    /// Filter to only metrics.
    pub fn metrics(self) -> Self {
        self.entry_type(EntryType::Metric)
    }

    /// Filter to only spans.
    pub fn spans(self) -> Self {
        self.entry_type(EntryType::Span)
    }

    /// Filter by service name.
    pub fn service(mut self, service: impl Into<String>) -> Self {
        self.filter.service = Some(service.into());
        self
    }

    /// Filter by trace ID.
    pub fn trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.filter.trace_id = Some(trace_id.into());
        self
    }

    /// Filter by metric name (only applies to metrics).
    pub fn metric_name(mut self, name: impl Into<String>) -> Self {
        self.filter.metric_name = Some(name.into());
        self
    }

    /// Filter by log level (only applies to logs).
    pub fn level(mut self, level: Level) -> Self {
        self.filter.level = Some(level);
        self
    }

    /// Filter by tag key-value pair.
    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.filter.tags.push((key.into(), value.into()));
        self
    }

    /// Limit the number of results.
    pub fn limit(mut self, limit: usize) -> Self {
        self.filter.limit = Some(limit);
        self
    }

    /// Execute the query and return matching entries.
    pub fn execute(self) -> Result<Vec<TelemetryEntry>> {
        // Determine query strategy based on filters
        let primary_keys = self.collect_candidate_keys()?;

        // Fetch and filter entries
        let mut results = Vec::new();
        let limit = self.filter.limit.unwrap_or(usize::MAX);

        for key_bytes in primary_keys {
            if results.len() >= limit {
                break;
            }

            if let Some(entry) = self.datapad.get_by_bytes(&key_bytes)? {
                if self.matches_filter(&entry) {
                    results.push(entry);
                }
            }
        }

        Ok(results)
    }

    /// Collect candidate primary keys based on available indices.
    fn collect_candidate_keys(&self) -> Result<Vec<Vec<u8>>> {
        let db = self.datapad.db();

        // If we have a trace_id filter, use the trace index
        if let Some(ref trace_id) = self.filter.trace_id {
            let trace_tree = db.open_tree("idx_trace")?;
            if let Some(keys) = trace_tree.get(trace_id.as_bytes())? {
                return Ok(extract_primary_keys(&keys));
            }
            return Ok(Vec::new());
        }

        // If we have a service filter and no time range, use service index
        if let Some(ref service) = self.filter.service {
            if self.filter.start_time.is_none() && self.filter.end_time.is_none() {
                let service_tree = db.open_tree("idx_service")?;
                if let Some(keys) = service_tree.get(service.as_bytes())? {
                    return Ok(extract_primary_keys(&keys));
                }
                return Ok(Vec::new());
            }
        }

        // If we have a metric_name filter, use metric index
        if let Some(ref metric_name) = self.filter.metric_name {
            let metric_tree = db.open_tree("idx_metric")?;
            if let Some(keys) = metric_tree.get(metric_name.as_bytes())? {
                return Ok(extract_primary_keys(&keys));
            }
            return Ok(Vec::new());
        }

        // If we have a level filter, use level index
        if let Some(level) = self.filter.level {
            let level_tree = db.open_tree("idx_level")?;
            if let Some(keys) = level_tree.get(&[level.as_u8()])? {
                return Ok(extract_primary_keys(&keys));
            }
            return Ok(Vec::new());
        }

        // Fall back to time range scan on primary tree
        let entries = db.open_tree("entries")?;
        let start = self
            .filter
            .start_time
            .map(PrimaryKey::range_start)
            .unwrap_or([0u8; PRIMARY_KEY_SIZE]);
        let end = self
            .filter
            .end_time
            .map(PrimaryKey::range_end)
            .unwrap_or([0xffu8; PRIMARY_KEY_SIZE]);

        let mut keys = Vec::new();
        for item in entries.range(start..=end) {
            let (key, _) = item?;
            keys.push(key.to_vec());
        }

        Ok(keys)
    }

    /// Check if an entry matches all filter criteria.
    fn matches_filter(&self, entry: &TelemetryEntry) -> bool {
        // Check time range
        if let Some(start) = self.filter.start_time {
            if entry.timestamp() < start {
                return false;
            }
        }
        if let Some(end) = self.filter.end_time {
            if entry.timestamp() > end {
                return false;
            }
        }

        // Check entry type
        if let Some(entry_type) = self.filter.entry_type {
            if entry.entry_type() != entry_type {
                return false;
            }
        }

        // Check service
        if let Some(ref service) = self.filter.service {
            if entry.service() != service {
                return false;
            }
        }

        // Check trace ID
        if let Some(ref trace_id) = self.filter.trace_id {
            match entry.trace_id() {
                Some(tid) if tid.as_str() == trace_id => {}
                _ => return false,
            }
        }

        // Check metric name
        if let Some(ref metric_name) = self.filter.metric_name {
            match entry {
                TelemetryEntry::Metric(m) if m.name == *metric_name => {}
                TelemetryEntry::Metric(_) => return false,
                _ => {} // Non-metrics pass if entry_type filter allows
            }
        }

        // Check log level
        if let Some(level) = self.filter.level {
            match entry {
                TelemetryEntry::Log(l) if l.level == level => {}
                TelemetryEntry::Log(_) => return false,
                _ => {} // Non-logs pass if entry_type filter allows
            }
        }

        // Check tags
        for (key, value) in &self.filter.tags {
            let tags = &entry.common().tags;
            match tags.get(key) {
                Some(v) if v == value => {}
                _ => return false,
            }
        }

        true
    }
}

/// Extract primary keys from index value (concatenated 25-byte keys).
fn extract_primary_keys(data: &[u8]) -> Vec<Vec<u8>> {
    data.chunks_exact(PRIMARY_KEY_SIZE)
        .map(|chunk| chunk.to_vec())
        .collect()
}

impl Datapad {
    /// Create a query builder.
    pub fn query(&self) -> QueryBuilder<'_> {
        QueryBuilder::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use constellation_telemetry::{CommonFields, LogEntry, MetricEntry, SpanEntry, TraceId};

    fn setup_test_data(datapad: &Datapad) {
        // Insert some test entries
        let common1 = CommonFields::new("auth", "node-1")
            .with_tag("env", "prod")
            .with_trace_id(TraceId::from_hex("0123456789abcdef0123456789abcdef").unwrap());
        let log1 = LogEntry::new(common1, Level::Info, "User logged in");

        let common2 = CommonFields::new("api", "node-1").with_tag("env", "staging");
        let log2 = LogEntry::new(common2, Level::Error, "Request failed");

        let common3 = CommonFields::new("auth", "node-2");
        let metric1 = MetricEntry::counter(common3, "login_count");

        let common4 =
            CommonFields::new("api", "node-1").with_trace_id(TraceId::from_hex("0123456789abcdef0123456789abcdef").unwrap());
        let span1 = SpanEntry::new(common4, "handle_request", 1000, 2000);

        datapad.insert(&log1.into()).unwrap();
        datapad.insert(&log2.into()).unwrap();
        datapad.insert(&metric1.into()).unwrap();
        datapad.insert(&span1.into()).unwrap();
    }

    #[test]
    fn query_all() {
        let datapad = Datapad::open_temporary().unwrap();
        setup_test_data(&datapad);

        let results = datapad.query().execute().unwrap();
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn query_by_service() {
        let datapad = Datapad::open_temporary().unwrap();
        setup_test_data(&datapad);

        let results = datapad.query().service("auth").execute().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_logs_only() {
        let datapad = Datapad::open_temporary().unwrap();
        setup_test_data(&datapad);

        let results = datapad.query().logs().execute().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_by_level() {
        let datapad = Datapad::open_temporary().unwrap();
        setup_test_data(&datapad);

        let results = datapad.query().level(Level::Error).execute().unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn query_by_trace_id() {
        let datapad = Datapad::open_temporary().unwrap();
        setup_test_data(&datapad);

        let results = datapad
            .query()
            .trace_id("0123456789abcdef0123456789abcdef")
            .execute()
            .unwrap();
        assert_eq!(results.len(), 2); // log1 and span1
    }

    #[test]
    fn query_by_tag() {
        let datapad = Datapad::open_temporary().unwrap();
        setup_test_data(&datapad);

        let results = datapad.query().tag("env", "prod").execute().unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn query_with_limit() {
        let datapad = Datapad::open_temporary().unwrap();
        setup_test_data(&datapad);

        let results = datapad.query().limit(2).execute().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_combined_filters() {
        let datapad = Datapad::open_temporary().unwrap();
        setup_test_data(&datapad);

        let results = datapad
            .query()
            .service("auth")
            .logs()
            .execute()
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn query_metrics_by_name() {
        let datapad = Datapad::open_temporary().unwrap();
        setup_test_data(&datapad);

        let results = datapad.query().metric_name("login_count").execute().unwrap();
        assert_eq!(results.len(), 1);
    }
}
