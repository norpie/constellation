use crate::{LogEntry, LogIndex, Result, Term};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Persistent state storage for Raft
///
/// All methods must be atomic - partial writes should never be visible.
/// Implementations should ensure durability before returning from save operations.
#[async_trait]
pub trait RaftStorage: Send + Sync {
    /// Save the current term
    ///
    /// Must be persisted before responding to any RPC.
    async fn save_term(&mut self, term: Term) -> Result<()>;

    /// Save which candidate received our vote in the current term
    ///
    /// Must be persisted before responding to RequestVote RPC.
    async fn save_voted_for(&mut self, voted_for: Option<String>) -> Result<()>;

    /// Append entries to the log
    ///
    /// Must be persisted before responding to AppendEntries RPC.
    async fn append_entries(&mut self, entries: Vec<LogEntry>) -> Result<()>;

    /// Delete log entries from index onwards
    ///
    /// Used when resolving log inconsistencies.
    async fn delete_entries_from(&mut self, index: LogIndex) -> Result<()>;

    /// Get the current term
    async fn get_term(&self) -> Result<Term>;

    /// Get who we voted for in the current term
    async fn get_voted_for(&self) -> Result<Option<String>>;

    /// Get all log entries
    async fn get_log(&self) -> Result<Vec<LogEntry>>;

    /// Get a log entry at a specific index
    ///
    /// Returns None if index is 0 or beyond the end of the log.
    async fn get_entry(&self, index: LogIndex) -> Result<Option<LogEntry>>;

    /// Get the index of the last log entry
    ///
    /// Returns 0 if log is empty.
    async fn last_log_index(&self) -> Result<LogIndex>;

    /// Get the term of the last log entry
    ///
    /// Returns 0 if log is empty.
    async fn last_log_term(&self) -> Result<Term>;
}

/// In-memory storage implementation (non-durable)
///
/// This is the default storage backend. State is lost on restart.
/// Suitable for testing or ephemeral clusters.
#[derive(Debug, Clone)]
pub struct MemoryStorage {
    inner: Arc<RwLock<MemoryStorageInner>>,
}

#[derive(Debug)]
struct MemoryStorageInner {
    term: Term,
    voted_for: Option<String>,
    log: Vec<LogEntry>,
}

impl MemoryStorage {
    /// Create a new in-memory storage
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MemoryStorageInner {
                term: 0,
                voted_for: None,
                log: Vec::new(),
            })),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RaftStorage for MemoryStorage {
    async fn save_term(&mut self, term: Term) -> Result<()> {
        let mut inner = self.inner.write().await;
        inner.term = term;
        Ok(())
    }

    async fn save_voted_for(&mut self, voted_for: Option<String>) -> Result<()> {
        let mut inner = self.inner.write().await;
        inner.voted_for = voted_for;
        Ok(())
    }

    async fn append_entries(&mut self, entries: Vec<LogEntry>) -> Result<()> {
        let mut inner = self.inner.write().await;
        inner.log.extend(entries);
        Ok(())
    }

    async fn delete_entries_from(&mut self, index: LogIndex) -> Result<()> {
        let mut inner = self.inner.write().await;
        if index > 0 && index as usize <= inner.log.len() {
            inner.log.truncate((index - 1) as usize);
        }
        Ok(())
    }

    async fn get_term(&self) -> Result<Term> {
        let inner = self.inner.read().await;
        Ok(inner.term)
    }

    async fn get_voted_for(&self) -> Result<Option<String>> {
        let inner = self.inner.read().await;
        Ok(inner.voted_for.clone())
    }

    async fn get_log(&self) -> Result<Vec<LogEntry>> {
        let inner = self.inner.read().await;
        Ok(inner.log.clone())
    }

    async fn get_entry(&self, index: LogIndex) -> Result<Option<LogEntry>> {
        if index == 0 {
            return Ok(None);
        }

        let inner = self.inner.read().await;
        Ok(inner.log.get((index - 1) as usize).cloned())
    }

    async fn last_log_index(&self) -> Result<LogIndex> {
        let inner = self.inner.read().await;
        Ok(inner.log.len() as LogIndex)
    }

    async fn last_log_term(&self) -> Result<Term> {
        let inner = self.inner.read().await;
        Ok(inner.log.last().map(|e| e.term).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_storage_basic() {
        let mut storage = MemoryStorage::new();

        // Initial state
        assert_eq!(storage.get_term().await.unwrap(), 0);
        assert_eq!(storage.get_voted_for().await.unwrap(), None);
        assert_eq!(storage.last_log_index().await.unwrap(), 0);
        assert_eq!(storage.last_log_term().await.unwrap(), 0);

        // Save term
        storage.save_term(5).await.unwrap();
        assert_eq!(storage.get_term().await.unwrap(), 5);

        // Save voted_for
        storage
            .save_voted_for(Some("node-1".to_string()))
            .await
            .unwrap();
        assert_eq!(
            storage.get_voted_for().await.unwrap(),
            Some("node-1".to_string())
        );
    }

    #[tokio::test]
    async fn test_memory_storage_log_operations() {
        let mut storage = MemoryStorage::new();

        // Append entries
        let entries = vec![
            LogEntry::new(1, vec![1, 2, 3]),
            LogEntry::new(1, vec![4, 5, 6]),
            LogEntry::new(2, vec![7, 8, 9]),
        ];
        storage.append_entries(entries.clone()).await.unwrap();

        assert_eq!(storage.last_log_index().await.unwrap(), 3);
        assert_eq!(storage.last_log_term().await.unwrap(), 2);

        // Get specific entry
        let entry = storage.get_entry(2).await.unwrap().unwrap();
        assert_eq!(entry.term, 1);
        assert_eq!(entry.command, vec![4, 5, 6]);

        // Get all entries
        let log = storage.get_log().await.unwrap();
        assert_eq!(log.len(), 3);

        // Delete from index 2 onwards
        storage.delete_entries_from(2).await.unwrap();
        assert_eq!(storage.last_log_index().await.unwrap(), 1);
        assert_eq!(storage.last_log_term().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_memory_storage_log_indexed_from_one() {
        let mut storage = MemoryStorage::new();

        let entries = vec![LogEntry::new(1, vec![42])];
        storage.append_entries(entries).await.unwrap();

        // Index 0 should return None
        assert!(storage.get_entry(0).await.unwrap().is_none());

        // Index 1 should return the first entry
        let entry = storage.get_entry(1).await.unwrap().unwrap();
        assert_eq!(entry.command, vec![42]);

        // Index 2 should return None (beyond end)
        assert!(storage.get_entry(2).await.unwrap().is_none());
    }
}
