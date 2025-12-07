//! Write-Ahead Log for telemetry overflow.
//!
//! When the in-memory collector buffer is full, entries spill to the WAL
//! instead of being dropped. The WAL provides durability and recovery.
//!
//! # File Format
//!
//! ```text
//! [HEADER: 8 bytes]
//!   Magic: 4 bytes "TWAL"
//!   Version: 2 bytes (u16 big-endian)
//!   Reserved: 2 bytes
//!
//! [ENTRY: variable] (repeated)
//!   Length: 4 bytes (u32 big-endian) - length of data only
//!   Checksum: 4 bytes (CRC32 of data)
//!   Data: bincode-encoded TelemetryEntry
//! ```

use crate::types::TelemetryEntry;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// WAL file magic bytes
const WAL_MAGIC: &[u8; 4] = b"TWAL";

/// Current WAL format version
const WAL_VERSION: u16 = 1;

/// Header size in bytes
const HEADER_SIZE: usize = 8;

/// Entry header size (length + checksum)
const ENTRY_HEADER_SIZE: usize = 8;

/// Default buffer size for writes
const DEFAULT_BUFFER_SIZE: usize = 64 * 1024; // 64KB

/// CRC32 polynomial (IEEE)
const CRC32_POLY: u32 = 0xEDB88320;

/// Simple CRC32 implementation
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ CRC32_POLY;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// WAL error types
#[derive(Debug)]
pub enum WalError {
    Io(io::Error),
    InvalidMagic,
    UnsupportedVersion(u16),
    ChecksumMismatch { expected: u32, actual: u32 },
    Serialize(String),
    Deserialize(String),
    TruncatedEntry,
}

impl std::fmt::Display for WalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalError::Io(e) => write!(f, "IO error: {}", e),
            WalError::InvalidMagic => write!(f, "Invalid WAL magic bytes"),
            WalError::UnsupportedVersion(v) => write!(f, "Unsupported WAL version: {}", v),
            WalError::ChecksumMismatch { expected, actual } => {
                write!(f, "Checksum mismatch: expected {}, got {}", expected, actual)
            }
            WalError::Serialize(e) => write!(f, "Serialization error: {}", e),
            WalError::Deserialize(e) => write!(f, "Deserialization error: {}", e),
            WalError::TruncatedEntry => write!(f, "Truncated entry in WAL"),
        }
    }
}

impl std::error::Error for WalError {}

impl From<io::Error> for WalError {
    fn from(e: io::Error) -> Self {
        WalError::Io(e)
    }
}

pub type WalResult<T> = Result<T, WalError>;

/// Configuration for WAL
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema, smart_default::SmartDefault)]
#[serde(default)]
pub struct WalConfig {
    /// Directory for WAL files
    #[default(PathBuf::from("./wal"))]
    pub dir: PathBuf,
    /// Maximum size per WAL file before rotation (bytes)
    #[default = 67108864] // 64MB
    pub max_file_size: u64,
    /// Maximum number of WAL files to keep
    #[default = 8]
    pub max_files: usize,
    /// Buffer size for writes
    #[default = 65536] // 64KB (DEFAULT_BUFFER_SIZE)
    pub buffer_size: usize,
    /// Sync after each write (slower but safer)
    #[default = false]
    pub sync_on_write: bool,
}

impl WalConfig {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            ..Default::default()
        }
    }

    pub fn max_file_size(mut self, size: u64) -> Self {
        self.max_file_size = size;
        self
    }

    pub fn max_files(mut self, count: usize) -> Self {
        self.max_files = count;
        self
    }

    pub fn sync_on_write(mut self, sync: bool) -> Self {
        self.sync_on_write = sync;
        self
    }
}

/// Single WAL file writer
pub struct Wal {
    writer: BufWriter<File>,
    path: PathBuf,
    position: u64,
    sync_on_write: bool,
}

impl Wal {
    /// Create a new WAL file
    pub fn create(path: impl AsRef<Path>) -> WalResult<Self> {
        Self::create_with_buffer(path, DEFAULT_BUFFER_SIZE, false)
    }

    /// Create a new WAL file with custom buffer size
    pub fn create_with_buffer(
        path: impl AsRef<Path>,
        buffer_size: usize,
        sync_on_write: bool,
    ) -> WalResult<Self> {
        let path = path.as_ref().to_path_buf();

        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;

        let mut writer = BufWriter::with_capacity(buffer_size, file);

        // Write header
        writer.write_all(WAL_MAGIC)?;
        writer.write_all(&WAL_VERSION.to_be_bytes())?;
        writer.write_all(&[0u8; 2])?; // reserved

        let position = HEADER_SIZE as u64;

        Ok(Self {
            writer,
            path,
            position,
            sync_on_write,
        })
    }

    /// Open an existing WAL file for appending
    pub fn open(path: impl AsRef<Path>) -> WalResult<Self> {
        Self::open_with_buffer(path, DEFAULT_BUFFER_SIZE, false)
    }

    /// Open an existing WAL file for appending with custom buffer size
    pub fn open_with_buffer(
        path: impl AsRef<Path>,
        buffer_size: usize,
        sync_on_write: bool,
    ) -> WalResult<Self> {
        let path = path.as_ref().to_path_buf();

        // First, validate the header
        {
            let mut file = File::open(&path)?;
            let mut header = [0u8; HEADER_SIZE];
            file.read_exact(&mut header)?;

            if &header[0..4] != WAL_MAGIC {
                return Err(WalError::InvalidMagic);
            }

            let version = u16::from_be_bytes([header[4], header[5]]);
            if version != WAL_VERSION {
                return Err(WalError::UnsupportedVersion(version));
            }
        }

        // Open for appending
        let file = OpenOptions::new().append(true).open(&path)?;
        let position = file.metadata()?.len();
        let writer = BufWriter::with_capacity(buffer_size, file);

        Ok(Self {
            writer,
            path,
            position,
            sync_on_write,
        })
    }

    /// Append an entry to the WAL
    pub fn append(&mut self, entry: &TelemetryEntry) -> WalResult<u64> {
        // Serialize entry
        let data = bincode::serde::encode_to_vec(entry, bincode::config::standard())
            .map_err(|e| WalError::Serialize(e.to_string()))?;

        let length = data.len() as u32;
        let checksum = crc32(&data);

        // Write entry header
        self.writer.write_all(&length.to_be_bytes())?;
        self.writer.write_all(&checksum.to_be_bytes())?;

        // Write data
        self.writer.write_all(&data)?;

        let entry_offset = self.position;
        self.position += ENTRY_HEADER_SIZE as u64 + data.len() as u64;

        if self.sync_on_write {
            self.sync()?;
        }

        Ok(entry_offset)
    }

    /// Flush buffered writes to the OS
    pub fn flush(&mut self) -> WalResult<()> {
        self.writer.flush()?;
        Ok(())
    }

    /// Sync writes to disk (fsync)
    pub fn sync(&mut self) -> WalResult<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        Ok(())
    }

    /// Get the current file size
    pub fn size(&self) -> u64 {
        self.position
    }

    /// Get the file path
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// WAL file reader
pub struct WalReader {
    reader: BufReader<File>,
    path: PathBuf,
}

impl WalReader {
    /// Open a WAL file for reading
    pub fn open(path: impl AsRef<Path>) -> WalResult<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;

        // Validate header
        let mut header = [0u8; HEADER_SIZE];
        file.read_exact(&mut header)?;

        if &header[0..4] != WAL_MAGIC {
            return Err(WalError::InvalidMagic);
        }

        let version = u16::from_be_bytes([header[4], header[5]]);
        if version != WAL_VERSION {
            return Err(WalError::UnsupportedVersion(version));
        }

        let reader = BufReader::new(file);

        Ok(Self { reader, path })
    }

    /// Seek to a specific offset
    pub fn seek(&mut self, offset: u64) -> WalResult<()> {
        self.reader.seek(SeekFrom::Start(offset))?;
        Ok(())
    }

    /// Read the next entry
    pub fn read_entry(&mut self) -> WalResult<Option<TelemetryEntry>> {
        // Read entry header
        let mut header = [0u8; ENTRY_HEADER_SIZE];
        match self.reader.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let expected_checksum = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

        // Read data
        let mut data = vec![0u8; length as usize];
        match self.reader.read_exact(&mut data) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                return Err(WalError::TruncatedEntry)
            }
            Err(e) => return Err(e.into()),
        }

        // Verify checksum
        let actual_checksum = crc32(&data);
        if actual_checksum != expected_checksum {
            return Err(WalError::ChecksumMismatch {
                expected: expected_checksum,
                actual: actual_checksum,
            });
        }

        // Deserialize
        let (entry, _): (TelemetryEntry, _) =
            bincode::serde::decode_from_slice(&data, bincode::config::standard())
                .map_err(|e| WalError::Deserialize(e.to_string()))?;

        Ok(Some(entry))
    }

    /// Create an iterator over entries
    pub fn iter(self) -> WalIterator {
        WalIterator { reader: self }
    }

    /// Get the file path
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Iterator over WAL entries
pub struct WalIterator {
    reader: WalReader,
}

impl Iterator for WalIterator {
    type Item = WalResult<TelemetryEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.reader.read_entry() {
            Ok(Some(entry)) => Some(Ok(entry)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// Manages multiple WAL files with rotation and cleanup
pub struct WalManager {
    config: WalConfig,
    current: Option<Wal>,
    /// Count of entries written (for metrics)
    entries_written: u64,
}

impl WalManager {
    /// Create a new WAL manager
    pub fn new(config: WalConfig) -> WalResult<Self> {
        // Ensure directory exists
        fs::create_dir_all(&config.dir)?;

        Ok(Self {
            config,
            current: None,
            entries_written: 0,
        })
    }

    /// Append an entry, rotating files as needed
    pub fn append(&mut self, entry: &TelemetryEntry) -> WalResult<()> {
        // Check if we need a new WAL file
        let needs_rotation = self
            .current
            .as_ref()
            .map(|w| w.size() >= self.config.max_file_size)
            .unwrap_or(true);

        if needs_rotation {
            self.rotate()?;
        }

        if let Some(ref mut wal) = self.current {
            wal.append(entry)?;
            self.entries_written += 1;
        }

        Ok(())
    }

    /// Rotate to a new WAL file
    pub fn rotate(&mut self) -> WalResult<()> {
        // Flush and sync current WAL if exists
        if let Some(ref mut current) = self.current {
            current.sync()?;
        }

        // Create new WAL file with timestamp-based name
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros();

        let filename = format!("wal_{}.log", timestamp);
        let path = self.config.dir.join(filename);

        self.current = Some(Wal::create_with_buffer(
            path,
            self.config.buffer_size,
            self.config.sync_on_write,
        )?);

        // Cleanup old files
        self.cleanup()?;

        Ok(())
    }

    /// Flush the current WAL
    pub fn flush(&mut self) -> WalResult<()> {
        if let Some(ref mut wal) = self.current {
            wal.flush()?;
        }
        Ok(())
    }

    /// Sync the current WAL to disk
    pub fn sync(&mut self) -> WalResult<()> {
        if let Some(ref mut wal) = self.current {
            wal.sync()?;
        }
        Ok(())
    }

    /// Remove old WAL files exceeding max_files limit
    pub fn cleanup(&mut self) -> WalResult<usize> {
        let mut wal_files: Vec<_> = fs::read_dir(&self.config.dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("wal_") && n.ends_with(".log"))
                    .unwrap_or(false)
            })
            .collect();

        // Sort by name (which includes timestamp, so oldest first)
        wal_files.sort_by_key(|e| e.path());

        let mut removed = 0;
        while wal_files.len() > self.config.max_files {
            if let Some(oldest) = wal_files.first() {
                // Don't remove the current WAL
                let is_current = self
                    .current
                    .as_ref()
                    .map(|w| w.path() == oldest.path())
                    .unwrap_or(false);

                if !is_current {
                    fs::remove_file(oldest.path())?;
                    removed += 1;
                }
            }
            wal_files.remove(0);
        }

        Ok(removed)
    }

    /// Recover all entries from WAL files
    pub fn recover(&self) -> WalResult<Vec<TelemetryEntry>> {
        let mut entries = Vec::new();

        let mut wal_files: Vec<_> = fs::read_dir(&self.config.dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("wal_") && n.ends_with(".log"))
                    .unwrap_or(false)
            })
            .collect();

        // Sort by name (oldest first)
        wal_files.sort_by_key(|e| e.path());

        for file in wal_files {
            match WalReader::open(file.path()) {
                Ok(reader) => {
                    for result in reader.iter() {
                        match result {
                            Ok(entry) => entries.push(entry),
                            Err(WalError::TruncatedEntry) => {
                                // Stop reading this file, it's incomplete
                                break;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }
                Err(WalError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                    // File was removed between listing and opening
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        Ok(entries)
    }

    /// Clear all WAL files (use after successful recovery)
    pub fn clear(&mut self) -> WalResult<usize> {
        // Close current WAL
        self.current = None;

        let mut removed = 0;
        for entry in fs::read_dir(&self.config.dir)? {
            let entry = entry?;
            if entry
                .path()
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("wal_") && n.ends_with(".log"))
                .unwrap_or(false)
            {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }

        Ok(removed)
    }

    /// Get the number of entries written
    pub fn entries_written(&self) -> u64 {
        self.entries_written
    }

    /// Get the current WAL file size
    pub fn current_size(&self) -> u64 {
        self.current.as_ref().map(|w| w.size()).unwrap_or(0)
    }

    /// Get the directory path
    pub fn dir(&self) -> &Path {
        &self.config.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CommonFields, Level, LogEntry, MetricEntry, SpanEntry};
    use std::sync::atomic::{AtomicU64, Ordering};
    use tempfile::TempDir;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_wal_path() -> PathBuf {
        let count = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("test_wal_{}.log", count))
    }

    fn make_log(service: &str, message: &str) -> TelemetryEntry {
        let common = CommonFields::new(service, "node-1");
        LogEntry::new(common, Level::Info, message).into()
    }

    fn make_metric(service: &str, name: &str) -> TelemetryEntry {
        let common = CommonFields::new(service, "node-1");
        MetricEntry::counter(common, name).into()
    }

    fn make_span(service: &str, name: &str) -> TelemetryEntry {
        let common = CommonFields::new(service, "node-1");
        SpanEntry::new(common, name, 1000, 2000).into()
    }

    #[test]
    fn crc32_basic() {
        // Known CRC32 values
        assert_eq!(crc32(b""), 0x00000000);
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn wal_create_and_append() {
        let path = temp_wal_path();
        let _cleanup = scopeguard::guard(path.clone(), |p| {
            let _ = fs::remove_file(p);
        });

        let mut wal = Wal::create(&path).unwrap();
        assert_eq!(wal.size(), HEADER_SIZE as u64);

        let entry = make_log("test", "hello world");
        let offset = wal.append(&entry).unwrap();
        assert_eq!(offset, HEADER_SIZE as u64);

        wal.sync().unwrap();
        assert!(wal.size() > HEADER_SIZE as u64);
    }

    #[test]
    fn wal_write_and_read() {
        let path = temp_wal_path();
        let _cleanup = scopeguard::guard(path.clone(), |p| {
            let _ = fs::remove_file(p);
        });

        // Write entries
        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(&make_log("svc1", "message 1")).unwrap();
            wal.append(&make_metric("svc2", "counter")).unwrap();
            wal.append(&make_span("svc3", "operation")).unwrap();
            wal.sync().unwrap();
        }

        // Read back
        let reader = WalReader::open(&path).unwrap();
        let entries: Vec<_> = reader.iter().collect();

        assert_eq!(entries.len(), 3);

        // Verify entries
        let entry0 = entries[0].as_ref().unwrap();
        assert_eq!(entry0.service(), "svc1");

        let entry1 = entries[1].as_ref().unwrap();
        assert_eq!(entry1.service(), "svc2");

        let entry2 = entries[2].as_ref().unwrap();
        assert_eq!(entry2.service(), "svc3");
    }

    #[test]
    fn wal_open_for_append() {
        let path = temp_wal_path();
        let _cleanup = scopeguard::guard(path.clone(), |p| {
            let _ = fs::remove_file(p);
        });

        // Create and write
        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(&make_log("svc", "first")).unwrap();
            wal.sync().unwrap();
        }

        // Open and append
        {
            let mut wal = Wal::open(&path).unwrap();
            wal.append(&make_log("svc", "second")).unwrap();
            wal.sync().unwrap();
        }

        // Read back
        let reader = WalReader::open(&path).unwrap();
        let entries: Vec<_> = reader.iter().collect();

        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn wal_invalid_magic() {
        let path = temp_wal_path();
        let _cleanup = scopeguard::guard(path.clone(), |p| {
            let _ = fs::remove_file(p);
        });

        // Write invalid header
        fs::write(&path, b"XXXX\x00\x01\x00\x00").unwrap();

        let result = WalReader::open(&path);
        assert!(matches!(result, Err(WalError::InvalidMagic)));
    }

    #[test]
    fn wal_unsupported_version() {
        let path = temp_wal_path();
        let _cleanup = scopeguard::guard(path.clone(), |p| {
            let _ = fs::remove_file(p);
        });

        // Write header with version 99
        fs::write(&path, b"TWAL\x00\x63\x00\x00").unwrap();

        let result = WalReader::open(&path);
        assert!(matches!(result, Err(WalError::UnsupportedVersion(99))));
    }

    #[test]
    fn wal_checksum_mismatch() {
        let path = temp_wal_path();
        let _cleanup = scopeguard::guard(path.clone(), |p| {
            let _ = fs::remove_file(p);
        });

        // Write valid entry
        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(&make_log("svc", "test")).unwrap();
            wal.sync().unwrap();
        }

        // Corrupt the data
        {
            let mut file = OpenOptions::new().write(true).open(&path).unwrap();
            file.seek(SeekFrom::Start(HEADER_SIZE as u64 + ENTRY_HEADER_SIZE as u64))
                .unwrap();
            file.write_all(b"CORRUPTED").unwrap();
        }

        let mut reader = WalReader::open(&path).unwrap();
        let result = reader.read_entry();
        assert!(matches!(result, Err(WalError::ChecksumMismatch { .. })));
    }

    #[test]
    fn wal_manager_basic() {
        let temp_dir = TempDir::new().unwrap();
        let config = WalConfig::new(temp_dir.path()).max_file_size(1024);

        let mut manager = WalManager::new(config).unwrap();

        // Append some entries
        for i in 0..10 {
            manager
                .append(&make_log("svc", &format!("message {}", i)))
                .unwrap();
        }
        manager.sync().unwrap();

        assert_eq!(manager.entries_written(), 10);

        // Recover entries
        let entries = manager.recover().unwrap();
        assert_eq!(entries.len(), 10);
    }

    #[test]
    fn wal_manager_rotation() {
        let temp_dir = TempDir::new().unwrap();
        // Very small file size to force rotation
        let config = WalConfig::new(temp_dir.path())
            .max_file_size(100)
            .max_files(3);

        let mut manager = WalManager::new(config).unwrap();

        // Append many entries to force multiple rotations
        for i in 0..20 {
            manager
                .append(&make_log("svc", &format!("message {}", i)))
                .unwrap();
        }
        manager.sync().unwrap();

        // Should have at most 3 WAL files
        let wal_files: Vec<_> = fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("wal_"))
                    .unwrap_or(false)
            })
            .collect();

        assert!(wal_files.len() <= 3);
    }

    #[test]
    fn wal_manager_clear() {
        let temp_dir = TempDir::new().unwrap();
        let config = WalConfig::new(temp_dir.path());

        let mut manager = WalManager::new(config).unwrap();

        // Write some entries
        for i in 0..5 {
            manager
                .append(&make_log("svc", &format!("msg {}", i)))
                .unwrap();
        }
        manager.sync().unwrap();

        // Clear
        let removed = manager.clear().unwrap();
        assert!(removed >= 1);

        // No entries should remain
        let entries = manager.recover().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn wal_large_entries() {
        let path = temp_wal_path();
        let _cleanup = scopeguard::guard(path.clone(), |p| {
            let _ = fs::remove_file(p);
        });

        // Create entry with large message
        let large_message = "x".repeat(100_000);
        let entry = make_log("svc", &large_message);

        // Write
        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(&entry).unwrap();
            wal.sync().unwrap();
        }

        // Read back
        let reader = WalReader::open(&path).unwrap();
        let entries: Vec<_> = reader.iter().collect();

        assert_eq!(entries.len(), 1);
        if let TelemetryEntry::Log(log) = entries[0].as_ref().unwrap() {
            assert_eq!(log.message.len(), 100_000);
        } else {
            panic!("Expected log entry");
        }
    }

    #[test]
    fn wal_mixed_entry_types() {
        let path = temp_wal_path();
        let _cleanup = scopeguard::guard(path.clone(), |p| {
            let _ = fs::remove_file(p);
        });

        let entries_to_write = vec![
            make_log("svc1", "log message"),
            make_metric("svc2", "counter"),
            make_span("svc3", "operation"),
            make_log("svc1", "another log"),
            make_metric("svc2", "gauge"),
        ];

        // Write
        {
            let mut wal = Wal::create(&path).unwrap();
            for entry in &entries_to_write {
                wal.append(entry).unwrap();
            }
            wal.sync().unwrap();
        }

        // Read back
        let reader = WalReader::open(&path).unwrap();
        let read_entries: Vec<_> = reader.iter().filter_map(|r| r.ok()).collect();

        assert_eq!(read_entries.len(), 5);

        // Verify types
        assert!(matches!(read_entries[0], TelemetryEntry::Log(_)));
        assert!(matches!(read_entries[1], TelemetryEntry::Metric(_)));
        assert!(matches!(read_entries[2], TelemetryEntry::Span(_)));
    }

    #[test]
    fn wal_recover_after_crash() {
        let temp_dir = TempDir::new().unwrap();
        let config = WalConfig::new(temp_dir.path());

        // Simulate writing without proper close
        {
            let mut manager = WalManager::new(config.clone()).unwrap();
            for i in 0..10 {
                manager
                    .append(&make_log("svc", &format!("msg {}", i)))
                    .unwrap();
            }
            // Note: no sync, simulating crash
            manager.flush().unwrap(); // At least flush to OS
        }

        // Recover with new manager
        let manager = WalManager::new(config).unwrap();
        let entries = manager.recover().unwrap();

        // Should have recovered the entries (may lose some due to no sync)
        assert!(!entries.is_empty());
    }
}
