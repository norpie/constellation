//! WAL Performance Benchmark
//!
//! Run with: cargo run --release --example wal_benchmark -p constellation-telemetry

use constellation_telemetry::types::{CommonFields, Level, LogEntry, TelemetryEntry};
use constellation_telemetry::wal::{Wal, WalConfig, WalManager, WalReader};
use constellation_telemetry::{BufferCollector, Collector, CollectorConfig};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn make_log(service: &str, message: &str) -> TelemetryEntry {
    let common = CommonFields::new(service, "node-1");
    LogEntry::new(common, Level::Info, message).into()
}

fn benchmark_wal_append(count: usize, sync_on_write: bool) -> Duration {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("bench.wal");

    let mut wal = Wal::create_with_buffer(&path, 64 * 1024, sync_on_write).unwrap();

    let entry = make_log("benchmark", "test message for performance measurement");

    let start = Instant::now();
    for _ in 0..count {
        wal.append(&entry).unwrap();
    }
    wal.sync().unwrap();
    start.elapsed()
}

fn benchmark_wal_read(count: usize) -> (Duration, Duration) {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("bench.wal");

    // Write entries first
    let entry = make_log("benchmark", "test message for performance measurement");
    {
        let mut wal = Wal::create(&path).unwrap();
        for _ in 0..count {
            wal.append(&entry).unwrap();
        }
        wal.sync().unwrap();
    }

    let write_size = std::fs::metadata(&path).unwrap().len();

    // Read entries
    let start = Instant::now();
    let reader = WalReader::open(&path).unwrap();
    let entries: Vec<_> = reader.iter().collect();
    let read_time = start.elapsed();

    assert_eq!(entries.len(), count);

    (read_time, Duration::from_secs_f64(write_size as f64 / 1_000_000.0))
}

fn benchmark_wal_manager(count: usize) -> Duration {
    let temp_dir = TempDir::new().unwrap();
    let config = WalConfig::new(temp_dir.path())
        .max_file_size(1024 * 1024) // 1MB per file
        .max_files(10);

    let mut manager = WalManager::new(config).unwrap();

    let entry = make_log("benchmark", "test message for performance measurement");

    let start = Instant::now();
    for _ in 0..count {
        manager.append(&entry).unwrap();
    }
    manager.sync().unwrap();
    start.elapsed()
}

fn benchmark_collector_without_wal(count: usize) -> Duration {
    let config = CollectorConfig::new(count);
    let collector = BufferCollector::new(config);

    let start = Instant::now();
    for i in 0..count {
        let common = CommonFields::new("benchmark", "node-1");
        let log = LogEntry::new(common, Level::Info, &format!("message {}", i));
        collector.collect_log(log);
    }
    start.elapsed()
}

fn benchmark_collector_with_wal(count: usize, buffer_size: usize) -> Duration {
    let temp_dir = TempDir::new().unwrap();
    let config = CollectorConfig::new(buffer_size).with_wal(temp_dir.path());
    let collector = BufferCollector::new(config);

    let start = Instant::now();
    for i in 0..count {
        let common = CommonFields::new("benchmark", "node-1");
        let log = LogEntry::new(common, Level::Info, &format!("message {}", i));
        collector.collect_log(log);
    }
    start.elapsed()
}

fn format_rate(count: usize, duration: Duration) -> String {
    let rate = count as f64 / duration.as_secs_f64();
    if rate >= 1_000_000.0 {
        format!("{:.2}M/s", rate / 1_000_000.0)
    } else if rate >= 1_000.0 {
        format!("{:.2}K/s", rate / 1_000.0)
    } else {
        format!("{:.2}/s", rate)
    }
}

fn main() {
    println!("=== WAL Performance Benchmark ===\n");

    let counts = [1_000, 10_000, 100_000];

    // WAL Append benchmarks
    println!("--- WAL Append (no sync per write) ---");
    for &count in &counts {
        let duration = benchmark_wal_append(count, false);
        println!(
            "  {:>7} entries: {:>8.2}ms ({:>10})",
            count,
            duration.as_secs_f64() * 1000.0,
            format_rate(count, duration)
        );
    }

    println!("\n--- WAL Append (sync per write) ---");
    for &count in &[100, 1_000] {
        let duration = benchmark_wal_append(count, true);
        println!(
            "  {:>7} entries: {:>8.2}ms ({:>10})",
            count,
            duration.as_secs_f64() * 1000.0,
            format_rate(count, duration)
        );
    }

    // WAL Read benchmarks
    println!("\n--- WAL Read ---");
    for &count in &counts {
        let (duration, _size) = benchmark_wal_read(count);
        println!(
            "  {:>7} entries: {:>8.2}ms ({:>10})",
            count,
            duration.as_secs_f64() * 1000.0,
            format_rate(count, duration)
        );
    }

    // WAL Manager with rotation
    println!("\n--- WAL Manager (with rotation) ---");
    for &count in &counts {
        let duration = benchmark_wal_manager(count);
        println!(
            "  {:>7} entries: {:>8.2}ms ({:>10})",
            count,
            duration.as_secs_f64() * 1000.0,
            format_rate(count, duration)
        );
    }

    // Collector benchmarks
    println!("\n--- Collector (in-memory only) ---");
    for &count in &counts {
        let duration = benchmark_collector_without_wal(count);
        println!(
            "  {:>7} entries: {:>8.2}ms ({:>10})",
            count,
            duration.as_secs_f64() * 1000.0,
            format_rate(count, duration)
        );
    }

    println!("\n--- Collector (with WAL overflow, buffer=1000) ---");
    for &count in &counts {
        let duration = benchmark_collector_with_wal(count, 1000);
        println!(
            "  {:>7} entries: {:>8.2}ms ({:>10})",
            count,
            duration.as_secs_f64() * 1000.0,
            format_rate(count, duration)
        );
    }

    println!("\n--- Collector (with WAL overflow, buffer=100) ---");
    for &count in &counts {
        let duration = benchmark_collector_with_wal(count, 100);
        println!(
            "  {:>7} entries: {:>8.2}ms ({:>10})",
            count,
            duration.as_secs_f64() * 1000.0,
            format_rate(count, duration)
        );
    }

    println!("\n=== Benchmark Complete ===");
}
