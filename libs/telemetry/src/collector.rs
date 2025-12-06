//! Telemetry collection and buffering.
//!
//! The collector receives telemetry entries (logs, metrics, spans) and buffers
//! them until they are scraped or pushed to the telemetry service.

use crate::types::{Level, LogEntry, MetricEntry, SpanEntry, TelemetryEntry};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

/// Global collector instance.
static GLOBAL_COLLECTOR: OnceLock<Arc<dyn Collector>> = OnceLock::new();

/// Trait for telemetry collectors.
///
/// Collectors receive telemetry entries and buffer them for later retrieval.
pub trait Collector: Send + Sync {
    /// Collect a log entry.
    fn collect_log(&self, entry: LogEntry);

    /// Collect a metric entry.
    fn collect_metric(&self, entry: MetricEntry);

    /// Collect a span entry.
    fn collect_span(&self, entry: SpanEntry);

    /// Collect any telemetry entry.
    fn collect(&self, entry: TelemetryEntry) {
        match entry {
            TelemetryEntry::Log(e) => self.collect_log(e),
            TelemetryEntry::Metric(e) => self.collect_metric(e),
            TelemetryEntry::Span(e) => self.collect_span(e),
        }
    }

    /// Drain all buffered entries.
    ///
    /// Returns the entries and clears the buffer.
    fn drain(&self) -> Vec<TelemetryEntry>;

    /// Get the current number of buffered entries.
    fn len(&self) -> usize;

    /// Check if the buffer is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Configuration for the buffer collector.
#[derive(Debug, Clone)]
pub struct CollectorConfig {
    /// Maximum number of entries to buffer before dropping oldest.
    pub max_entries: usize,

    /// Log levels that should trigger immediate push (not just buffering).
    /// Useful for ensuring errors are sent quickly.
    pub immediate_levels: HashSet<Level>,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            immediate_levels: HashSet::from([Level::Error]),
        }
    }
}

impl CollectorConfig {
    /// Create a new config with specified max entries.
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            ..Default::default()
        }
    }

    /// Set the immediate push levels.
    pub fn with_immediate_levels(mut self, levels: impl IntoIterator<Item = Level>) -> Self {
        self.immediate_levels = levels.into_iter().collect();
        self
    }
}

/// In-memory ring buffer collector.
///
/// Stores entries up to `max_entries`, dropping oldest when full.
pub struct BufferCollector {
    config: CollectorConfig,
    buffer: Mutex<RingBuffer>,
    /// Callback for immediate push (e.g., for error logs)
    immediate_callback: Mutex<Option<Box<dyn Fn(&TelemetryEntry) + Send + Sync>>>,
}

struct RingBuffer {
    entries: Vec<TelemetryEntry>,
    max_size: usize,
}

impl RingBuffer {
    fn new(max_size: usize) -> Self {
        Self {
            entries: Vec::with_capacity(max_size.min(1024)), // Don't pre-allocate huge buffers
            max_size,
        }
    }

    fn push(&mut self, entry: TelemetryEntry) {
        if self.entries.len() >= self.max_size {
            // Drop oldest entry
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    fn drain(&mut self) -> Vec<TelemetryEntry> {
        std::mem::take(&mut self.entries)
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

impl BufferCollector {
    /// Create a new buffer collector with the given config.
    pub fn new(config: CollectorConfig) -> Self {
        let max_size = config.max_entries;
        Self {
            config,
            buffer: Mutex::new(RingBuffer::new(max_size)),
            immediate_callback: Mutex::new(None),
        }
    }

    /// Create with default config.
    pub fn with_defaults() -> Self {
        Self::new(CollectorConfig::default())
    }

    /// Set a callback for immediate push entries.
    ///
    /// This callback is invoked for entries that match `immediate_levels`.
    pub fn on_immediate<F>(&self, callback: F)
    where
        F: Fn(&TelemetryEntry) + Send + Sync + 'static,
    {
        let mut cb = self.immediate_callback.lock().unwrap();
        *cb = Some(Box::new(callback));
    }

    /// Check if a log level should trigger immediate push.
    fn should_push_immediate(&self, level: Level) -> bool {
        self.config.immediate_levels.contains(&level)
    }

    /// Internal collect with optional immediate callback.
    fn collect_internal(&self, entry: TelemetryEntry, check_immediate: Option<Level>) {
        // Check for immediate push
        if let Some(level) = check_immediate {
            if self.should_push_immediate(level) {
                if let Ok(cb) = self.immediate_callback.lock() {
                    if let Some(ref callback) = *cb {
                        callback(&entry);
                    }
                }
            }
        }

        // Always buffer
        let mut buffer = self.buffer.lock().unwrap();
        buffer.push(entry);
    }
}

impl Collector for BufferCollector {
    fn collect_log(&self, entry: LogEntry) {
        let level = entry.level;
        self.collect_internal(TelemetryEntry::Log(entry), Some(level));
    }

    fn collect_metric(&self, entry: MetricEntry) {
        self.collect_internal(TelemetryEntry::Metric(entry), None);
    }

    fn collect_span(&self, entry: SpanEntry) {
        self.collect_internal(TelemetryEntry::Span(entry), None);
    }

    fn drain(&self) -> Vec<TelemetryEntry> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.drain()
    }

    fn len(&self) -> usize {
        let buffer = self.buffer.lock().unwrap();
        buffer.len()
    }
}

// === Global Collector API ===

/// Set the global collector.
///
/// This should be called once at application startup.
/// Returns `Err` if a collector was already set.
pub fn set_global_collector(collector: Arc<dyn Collector>) -> Result<(), Arc<dyn Collector>> {
    GLOBAL_COLLECTOR.set(collector)
}

/// Get the global collector, if set.
pub fn global_collector() -> Option<&'static Arc<dyn Collector>> {
    GLOBAL_COLLECTOR.get()
}

/// Collect a telemetry entry using the global collector.
///
/// No-op if no global collector is set.
pub fn collect(entry: TelemetryEntry) {
    if let Some(collector) = global_collector() {
        collector.collect(entry);
    }
}

/// Collect a log entry using the global collector.
pub fn collect_log(entry: LogEntry) {
    if let Some(collector) = global_collector() {
        collector.collect_log(entry);
    }
}

/// Collect a metric entry using the global collector.
pub fn collect_metric(entry: MetricEntry) {
    if let Some(collector) = global_collector() {
        collector.collect_metric(entry);
    }
}

/// Collect a span entry using the global collector.
pub fn collect_span(entry: SpanEntry) {
    if let Some(collector) = global_collector() {
        collector.collect_span(entry);
    }
}

/// Drain all entries from the global collector.
///
/// Returns empty vec if no collector is set.
pub fn drain() -> Vec<TelemetryEntry> {
    global_collector()
        .map(|c| c.drain())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CommonFields;

    fn make_log(service: &str, level: Level, message: &str) -> LogEntry {
        let common = CommonFields::new(service, "node-1");
        LogEntry::new(common, level, message)
    }

    fn make_metric(service: &str, name: &str) -> MetricEntry {
        let common = CommonFields::new(service, "node-1");
        MetricEntry::counter(common, name)
    }

    fn make_span(service: &str, name: &str) -> SpanEntry {
        let common = CommonFields::new(service, "node-1");
        SpanEntry::new(common, name, 1000, 2000)
    }

    #[test]
    fn buffer_collector_basic() {
        let collector = BufferCollector::with_defaults();

        collector.collect_log(make_log("auth", Level::Info, "hello"));
        collector.collect_metric(make_metric("auth", "requests"));
        collector.collect_span(make_span("auth", "handle"));

        assert_eq!(collector.len(), 3);

        let entries = collector.drain();
        assert_eq!(entries.len(), 3);
        assert!(collector.is_empty());
    }

    #[test]
    fn buffer_collector_ring_behavior() {
        let config = CollectorConfig::new(3);
        let collector = BufferCollector::new(config);

        // Add 5 entries to a buffer of size 3
        collector.collect_log(make_log("a", Level::Info, "1"));
        collector.collect_log(make_log("b", Level::Info, "2"));
        collector.collect_log(make_log("c", Level::Info, "3"));
        collector.collect_log(make_log("d", Level::Info, "4"));
        collector.collect_log(make_log("e", Level::Info, "5"));

        assert_eq!(collector.len(), 3);

        let entries = collector.drain();
        assert_eq!(entries.len(), 3);

        // Should have entries 3, 4, 5 (oldest dropped)
        let messages: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                TelemetryEntry::Log(l) => Some(l.message.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(messages, vec!["3", "4", "5"]);
    }

    #[test]
    fn immediate_callback() {
        let collector = BufferCollector::with_defaults();
        let immediate_count = Arc::new(Mutex::new(0));
        let count_clone = immediate_count.clone();

        collector.on_immediate(move |_| {
            let mut count = count_clone.lock().unwrap();
            *count += 1;
        });

        // Info should not trigger immediate
        collector.collect_log(make_log("auth", Level::Info, "normal"));
        assert_eq!(*immediate_count.lock().unwrap(), 0);

        // Error should trigger immediate
        collector.collect_log(make_log("auth", Level::Error, "error!"));
        assert_eq!(*immediate_count.lock().unwrap(), 1);

        // Both should be buffered
        assert_eq!(collector.len(), 2);
    }

    #[test]
    fn custom_immediate_levels() {
        let config = CollectorConfig::default()
            .with_immediate_levels([Level::Error, Level::Warn]);
        let collector = BufferCollector::new(config);
        let immediate_count = Arc::new(Mutex::new(0));
        let count_clone = immediate_count.clone();

        collector.on_immediate(move |_| {
            let mut count = count_clone.lock().unwrap();
            *count += 1;
        });

        collector.collect_log(make_log("a", Level::Debug, "debug"));
        collector.collect_log(make_log("a", Level::Info, "info"));
        collector.collect_log(make_log("a", Level::Warn, "warn"));
        collector.collect_log(make_log("a", Level::Error, "error"));

        // Warn and Error should trigger
        assert_eq!(*immediate_count.lock().unwrap(), 2);
    }

    #[test]
    fn collector_trait_collect_dispatch() {
        let collector = BufferCollector::with_defaults();

        let log: TelemetryEntry = make_log("a", Level::Info, "test").into();
        let metric: TelemetryEntry = make_metric("a", "count").into();
        let span: TelemetryEntry = make_span("a", "op").into();

        collector.collect(log);
        collector.collect(metric);
        collector.collect(span);

        let entries = collector.drain();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn drain_clears_buffer() {
        let collector = BufferCollector::with_defaults();

        collector.collect_log(make_log("a", Level::Info, "1"));
        collector.collect_log(make_log("a", Level::Info, "2"));

        assert_eq!(collector.len(), 2);

        let first = collector.drain();
        assert_eq!(first.len(), 2);
        assert!(collector.is_empty());

        let second = collector.drain();
        assert!(second.is_empty());
    }

    #[test]
    fn concurrent_access() {
        use std::thread;

        let collector = Arc::new(BufferCollector::new(CollectorConfig::new(1000)));
        let mut handles = vec![];

        // Spawn 10 threads, each adding 100 entries
        for i in 0..10 {
            let c = collector.clone();
            handles.push(thread::spawn(move || {
                for j in 0..100 {
                    c.collect_log(make_log(&format!("svc-{}", i), Level::Info, &format!("msg-{}", j)));
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(collector.len(), 1000);
    }
}
