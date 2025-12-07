use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::CommonFields;

/// Log severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }

    pub fn as_u8(&self) -> u8 {
        match self {
            Level::Debug => 0,
            Level::Info => 1,
            Level::Warn => 2,
            Level::Error => 3,
        }
    }

    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Level::Debug),
            1 => Some(Level::Info),
            2 => Some(Level::Warn),
            3 => Some(Level::Error),
            _ => None,
        }
    }
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A log entry
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LogEntry {
    /// Common fields (id, timestamp, service, node_id, trace_id, span_id, tags)
    pub common: CommonFields,

    /// Log severity level
    pub level: Level,

    /// Log message
    pub message: String,

    /// Module path or logger name (e.g., "my_crate::handlers::auth")
    pub target: Option<String>,

    /// Source file where the log was generated
    pub file: Option<String>,

    /// Line number in the source file
    pub line: Option<u32>,
}

impl LogEntry {
    /// Create a new log entry
    pub fn new(common: CommonFields, level: Level, message: impl Into<String>) -> Self {
        Self {
            common,
            level,
            message: message.into(),
            target: None,
            file: None,
            line: None,
        }
    }

    /// Set source location
    pub fn with_location(
        mut self,
        target: impl Into<String>,
        file: impl Into<String>,
        line: u32,
    ) -> Self {
        self.target = Some(target.into());
        self.file = Some(file.into());
        self.line = Some(line);
        self
    }

    /// Set target only
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_ordering() {
        assert!(Level::Debug < Level::Info);
        assert!(Level::Info < Level::Warn);
        assert!(Level::Warn < Level::Error);
    }

    #[test]
    fn log_entry_creation() {
        let common = CommonFields::new("auth", "auth-1");
        let entry = LogEntry::new(common, Level::Info, "User logged in")
            .with_location("auth::handlers", "src/handlers.rs", 42);

        assert_eq!(entry.level, Level::Info);
        assert_eq!(entry.message, "User logged in");
        assert_eq!(entry.file, Some("src/handlers.rs".to_string()));
        assert_eq!(entry.line, Some(42));
    }
}
