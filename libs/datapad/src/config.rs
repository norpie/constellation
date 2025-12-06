//! Datapad configuration.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

/// Configuration for opening a Datapad instance.
#[derive(Debug, Clone)]
pub struct DatapadConfig {
    /// Storage mode (file path or temporary).
    pub storage: StorageMode,

    /// Tags to create secondary indices for.
    /// These enable fast filtering by tag key-value pairs.
    pub indexed_tags: HashSet<String>,

    /// Retention configuration per entry type.
    pub retention: RetentionConfig,
}

/// Where to store the database.
#[derive(Debug, Clone)]
pub enum StorageMode {
    /// Persistent storage at the given path.
    Path(PathBuf),

    /// Temporary in-memory storage (for testing).
    Temporary,
}

/// Retention durations per entry type.
#[derive(Debug, Clone)]
pub struct RetentionConfig {
    /// How long to keep log entries.
    pub logs: Duration,

    /// How long to keep metric entries.
    pub metrics: Duration,

    /// How long to keep span entries.
    pub spans: Duration,

    /// How long to keep aggregated rollups (longer than raw).
    pub rollups: Duration,
}

impl Default for DatapadConfig {
    fn default() -> Self {
        Self {
            storage: StorageMode::Temporary,
            indexed_tags: HashSet::new(),
            retention: RetentionConfig::default(),
        }
    }
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            // 7 days for raw data
            logs: Duration::from_secs(7 * 24 * 60 * 60),
            metrics: Duration::from_secs(7 * 24 * 60 * 60),
            spans: Duration::from_secs(7 * 24 * 60 * 60),
            // 90 days for rollups
            rollups: Duration::from_secs(90 * 24 * 60 * 60),
        }
    }
}

impl DatapadConfig {
    /// Create config with a file path.
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self {
            storage: StorageMode::Path(path.into()),
            ..Default::default()
        }
    }

    /// Create config for temporary storage.
    pub fn temporary() -> Self {
        Self {
            storage: StorageMode::Temporary,
            ..Default::default()
        }
    }

    /// Add a tag to be indexed.
    pub fn index_tag(mut self, tag: impl Into<String>) -> Self {
        self.indexed_tags.insert(tag.into());
        self
    }

    /// Set retention config.
    pub fn with_retention(mut self, retention: RetentionConfig) -> Self {
        self.retention = retention;
        self
    }
}

impl RetentionConfig {
    /// Create with custom durations.
    pub fn new(logs: Duration, metrics: Duration, spans: Duration, rollups: Duration) -> Self {
        Self {
            logs,
            metrics,
            spans,
            rollups,
        }
    }
}
