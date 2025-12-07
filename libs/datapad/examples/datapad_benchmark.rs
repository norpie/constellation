//! Datapad Performance Benchmark
//!
//! Run with: cargo run --release --example datapad_benchmark -p constellation-datapad

use constellation_datapad::{Datapad, RetentionConfig};
use constellation_telemetry::{
    current_timestamp_micros, CommonFields, Level, LogEntry, MetricEntry, SpanEntry,
    TelemetryEntry, TraceId,
};
use std::time::{Duration, Instant};

fn make_log(service: &str, level: Level, message: &str) -> TelemetryEntry {
    let common = CommonFields::new(service, "node-1");
    LogEntry::new(common, level, message).into()
}

fn make_log_with_trace(service: &str, trace_id: &TraceId) -> TelemetryEntry {
    let common = CommonFields::new(service, "node-1").with_trace_id(trace_id.clone());
    LogEntry::new(common, Level::Info, "traced log").into()
}

fn make_metric(service: &str, name: &str, value: f64) -> TelemetryEntry {
    let common = CommonFields::new(service, "node-1");
    MetricEntry::gauge(common, name, value).into()
}

fn make_span(service: &str, name: &str, trace_id: &TraceId) -> TelemetryEntry {
    let common = CommonFields::new(service, "node-1").with_trace_id(trace_id.clone());
    SpanEntry::new(common, name, 1000, 2000).into()
}

fn benchmark_single_insert(count: usize) -> Duration {
    let datapad = Datapad::open_temporary().unwrap();

    let start = Instant::now();
    for i in 0..count {
        let entry = make_log("benchmark", Level::Info, &format!("message {}", i));
        datapad.insert(&entry).unwrap();
    }
    datapad.flush().unwrap();
    start.elapsed()
}

fn benchmark_batch_insert(count: usize, batch_size: usize) -> Duration {
    let datapad = Datapad::open_temporary().unwrap();

    let start = Instant::now();
    for batch_start in (0..count).step_by(batch_size) {
        let batch: Vec<_> = (batch_start..batch_start + batch_size.min(count - batch_start))
            .map(|i| make_log("benchmark", Level::Info, &format!("message {}", i)))
            .collect();
        datapad.insert_batch(&batch).unwrap();
    }
    datapad.flush().unwrap();
    start.elapsed()
}

fn benchmark_query_by_service(entry_count: usize, query_count: usize) -> Duration {
    let datapad = Datapad::open_temporary().unwrap();

    // Insert entries with different services
    let services = ["auth", "api", "worker", "gateway"];
    for i in 0..entry_count {
        let service = services[i % services.len()];
        let entry = make_log(service, Level::Info, &format!("message {}", i));
        datapad.insert(&entry).unwrap();
    }
    datapad.flush().unwrap();

    let start = Instant::now();
    for _ in 0..query_count {
        let results = datapad.query().service("auth").execute().unwrap();
        std::hint::black_box(results);
    }
    start.elapsed()
}

fn benchmark_query_by_trace_id(entry_count: usize, query_count: usize) -> Duration {
    let datapad = Datapad::open_temporary().unwrap();

    // Create some trace IDs and insert entries
    let trace_ids: Vec<_> = (0..10).map(|_| TraceId::new()).collect();
    for i in 0..entry_count {
        let trace_id = &trace_ids[i % trace_ids.len()];
        let entry = make_log_with_trace("benchmark", trace_id);
        datapad.insert(&entry).unwrap();
    }
    datapad.flush().unwrap();

    let target_trace = &trace_ids[0];
    let start = Instant::now();
    for _ in 0..query_count {
        let results = datapad
            .query()
            .trace_id(target_trace.as_str())
            .execute()
            .unwrap();
        std::hint::black_box(results);
    }
    start.elapsed()
}

fn benchmark_query_by_metric_name(entry_count: usize, query_count: usize) -> Duration {
    let datapad = Datapad::open_temporary().unwrap();

    // Insert metrics with different names
    let metric_names = ["requests", "latency", "errors", "connections"];
    for i in 0..entry_count {
        let name = metric_names[i % metric_names.len()];
        let entry = make_metric("benchmark", name, i as f64);
        datapad.insert(&entry).unwrap();
    }
    datapad.flush().unwrap();

    let start = Instant::now();
    for _ in 0..query_count {
        let results = datapad.query().metric_name("requests").execute().unwrap();
        std::hint::black_box(results);
    }
    start.elapsed()
}

fn benchmark_query_by_level(entry_count: usize, query_count: usize) -> Duration {
    let datapad = Datapad::open_temporary().unwrap();

    // Insert logs with different levels
    let levels = [Level::Debug, Level::Info, Level::Warn, Level::Error];
    for i in 0..entry_count {
        let level = levels[i % levels.len()];
        let entry = make_log("benchmark", level, &format!("message {}", i));
        datapad.insert(&entry).unwrap();
    }
    datapad.flush().unwrap();

    let start = Instant::now();
    for _ in 0..query_count {
        let results = datapad.query().level(Level::Error).execute().unwrap();
        std::hint::black_box(results);
    }
    start.elapsed()
}

fn benchmark_time_range_query(entry_count: usize, query_count: usize) -> Duration {
    let datapad = Datapad::open_temporary().unwrap();

    // Insert entries
    for i in 0..entry_count {
        let entry = make_log("benchmark", Level::Info, &format!("message {}", i));
        datapad.insert(&entry).unwrap();
    }
    datapad.flush().unwrap();

    let now = current_timestamp_micros();
    let one_hour_ago = now - 3600 * 1_000_000;

    let start = Instant::now();
    for _ in 0..query_count {
        let results = datapad
            .query()
            .time_range(one_hour_ago, now)
            .execute()
            .unwrap();
        std::hint::black_box(results);
    }
    start.elapsed()
}

fn benchmark_mixed_entries(count: usize) -> Duration {
    let datapad = Datapad::open_temporary().unwrap();
    let trace_id = TraceId::new();

    let start = Instant::now();
    for i in 0..count {
        let entry = match i % 3 {
            0 => make_log("api", Level::Info, &format!("request {}", i)),
            1 => make_metric("api", "request_count", i as f64),
            _ => make_span("api", "handle_request", &trace_id),
        };
        datapad.insert(&entry).unwrap();
    }
    datapad.flush().unwrap();
    start.elapsed()
}

fn benchmark_retention_cleanup(entry_count: usize) -> Duration {
    let datapad = Datapad::open_temporary().unwrap();

    let now = current_timestamp_micros();
    let day = 24 * 60 * 60 * 1_000_000u64;

    // Insert entries with various ages
    for i in 0..entry_count {
        let age_days = (i % 14) as u64; // 0-13 days old
        let mut common = CommonFields::new("benchmark", "node-1");
        common.timestamp = now - age_days * day;
        let entry: TelemetryEntry = LogEntry::new(common, Level::Info, "old log").into();
        datapad.insert(&entry).unwrap();
    }
    datapad.flush().unwrap();

    let config = RetentionConfig {
        logs: Duration::from_secs(7 * 24 * 60 * 60),    // 7 days
        metrics: Duration::from_secs(7 * 24 * 60 * 60), // 7 days
        spans: Duration::from_secs(7 * 24 * 60 * 60),   // 7 days
        rollups: Duration::from_secs(90 * 24 * 60 * 60),
    };

    let start = Instant::now();
    let result = datapad.cleanup(&config).unwrap();
    let elapsed = start.elapsed();

    println!(
        "    (deleted {} entries, {} remaining)",
        result.total(),
        datapad.count().unwrap()
    );
    elapsed
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

fn format_query_rate(count: usize, duration: Duration) -> String {
    let rate = count as f64 / duration.as_secs_f64();
    if rate >= 1_000.0 {
        format!("{:.2}K queries/s", rate / 1_000.0)
    } else {
        format!("{:.2} queries/s", rate)
    }
}

fn main() {
    println!("=== Datapad Performance Benchmark ===\n");

    // Insert benchmarks
    println!("--- Single Insert ---");
    for &count in &[1_000, 10_000, 50_000] {
        let duration = benchmark_single_insert(count);
        println!(
            "  {:>6} entries: {:>8.2}ms ({:>10})",
            count,
            duration.as_secs_f64() * 1000.0,
            format_rate(count, duration)
        );
    }

    println!("\n--- Batch Insert (batch size 100) ---");
    for &count in &[1_000, 10_000, 50_000] {
        let duration = benchmark_batch_insert(count, 100);
        println!(
            "  {:>6} entries: {:>8.2}ms ({:>10})",
            count,
            duration.as_secs_f64() * 1000.0,
            format_rate(count, duration)
        );
    }

    println!("\n--- Mixed Entry Types Insert ---");
    for &count in &[1_000, 10_000, 50_000] {
        let duration = benchmark_mixed_entries(count);
        println!(
            "  {:>6} entries: {:>8.2}ms ({:>10})",
            count,
            duration.as_secs_f64() * 1000.0,
            format_rate(count, duration)
        );
    }

    // Query benchmarks (10k entries, 1k queries)
    let entry_count = 10_000;
    let query_count = 1_000;

    println!("\n--- Query by Service (10k entries, 1k queries) ---");
    let duration = benchmark_query_by_service(entry_count, query_count);
    println!(
        "  {:>8.2}ms total ({:>15})",
        duration.as_secs_f64() * 1000.0,
        format_query_rate(query_count, duration)
    );

    println!("\n--- Query by Trace ID (10k entries, 1k queries) ---");
    let duration = benchmark_query_by_trace_id(entry_count, query_count);
    println!(
        "  {:>8.2}ms total ({:>15})",
        duration.as_secs_f64() * 1000.0,
        format_query_rate(query_count, duration)
    );

    println!("\n--- Query by Metric Name (10k entries, 1k queries) ---");
    let duration = benchmark_query_by_metric_name(entry_count, query_count);
    println!(
        "  {:>8.2}ms total ({:>15})",
        duration.as_secs_f64() * 1000.0,
        format_query_rate(query_count, duration)
    );

    println!("\n--- Query by Log Level (10k entries, 1k queries) ---");
    let duration = benchmark_query_by_level(entry_count, query_count);
    println!(
        "  {:>8.2}ms total ({:>15})",
        duration.as_secs_f64() * 1000.0,
        format_query_rate(query_count, duration)
    );

    println!("\n--- Time Range Query (10k entries, 1k queries) ---");
    let duration = benchmark_time_range_query(entry_count, query_count);
    println!(
        "  {:>8.2}ms total ({:>15})",
        duration.as_secs_f64() * 1000.0,
        format_query_rate(query_count, duration)
    );

    // Retention cleanup benchmark
    println!("\n--- Retention Cleanup ---");
    for &count in &[1_000, 10_000, 50_000] {
        print!("  {:>6} entries: ", count);
        let duration = benchmark_retention_cleanup(count);
        println!(
            "{:>8.2}ms",
            duration.as_secs_f64() * 1000.0,
        );
    }

    println!("\n=== Benchmark Complete ===");
}
