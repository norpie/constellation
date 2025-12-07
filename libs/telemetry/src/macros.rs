//! Telemetry macros for ergonomic logging, metrics, and spans.
//!
//! These macros automatically capture source location and integrate with
//! the telemetry context and collector.

/// Internal macro to create a log entry with context and source location.
#[macro_export]
macro_rules! __log_internal {
    ($level:expr, $msg:expr $(, $key:expr => $val:expr)* $(,)?) => {{
        if let Some(entry) = $crate::TelemetryContext::with(|ctx| {
            let mut common = $crate::CommonFields::new(&ctx.service, &ctx.node_id);
            common.trace_id = Some(ctx.trace_id.clone());
            common.span_id = Some(ctx.span_id.clone());
            $(
                common.tags.insert($key.into(), $val.to_string());
            )*
            let mut log = $crate::LogEntry::new(common, $level, $msg);
            log.file = Some(file!().to_string());
            log.line = Some(line!());
            log
        }) {
            $crate::collect_log(entry);
        }
    }};
}

/// Log an error message.
///
/// # Examples
///
/// ```text
/// error!("Connection failed");
/// error!("Request failed: {}", err);
/// error!("Auth error", "user_id" => user_id, "reason" => "invalid_token");
/// ```
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::__log_impl!($crate::Level::Error, $($arg)*)
    };
}

/// Log a warning message.
///
/// # Examples
///
/// ```text
/// warn!("Slow query detected");
/// warn!("High memory usage: {}%", usage);
/// warn!("Rate limit approaching", "current" => count, "limit" => max);
/// ```
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::__log_impl!($crate::Level::Warn, $($arg)*)
    };
}

/// Log an info message.
///
/// # Examples
///
/// ```text
/// info!("Server started");
/// info!("User {} logged in", user_id);
/// info!("Request handled", "method" => "GET", "path" => path);
/// ```
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::__log_impl!($crate::Level::Info, $($arg)*)
    };
}

/// Log a debug message.
///
/// # Examples
///
/// ```text
/// debug!("Entering function");
/// debug!("Cache hit for key: {}", key);
/// debug!("Query plan", "tables" => tables.join(","));
/// ```
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::__log_impl!($crate::Level::Debug, $($arg)*)
    };
}

/// Internal implementation macro that handles all log formats.
#[macro_export]
#[doc(hidden)]
macro_rules! __log_impl {
    // Format string with args, then tags: info!("msg {}", arg, "key" => val)
    ($level:expr, $fmt:expr, $($arg:expr),+ , $($key:expr => $val:expr),+ $(,)?) => {{
        let msg = format!($fmt, $($arg),+);
        $crate::__log_internal!($level, msg $(, $key => $val)+);
    }};

    // Just format string with args: info!("msg {}", arg)
    ($level:expr, $fmt:expr, $($arg:expr),+ $(,)?) => {{
        // Check if args contain "=>" (are tags) or not (are format args)
        $crate::__log_impl!(@check $level, $fmt, $($arg),+)
    }};

    // Check helper - if second arg is a tag pattern
    (@check $level:expr, $fmt:expr, $key:expr => $val:expr $(, $rest_key:expr => $rest_val:expr)* $(,)?) => {{
        $crate::__log_internal!($level, $fmt, $key => $val $(, $rest_key => $rest_val)*);
    }};

    // Check helper - if args are format args
    (@check $level:expr, $fmt:expr, $($arg:expr),+ $(,)?) => {{
        let msg = format!($fmt, $($arg),+);
        $crate::__log_internal!($level, msg);
    }};

    // Just message with tags: info!("msg", "key" => val)
    ($level:expr, $msg:expr, $($key:expr => $val:expr),+ $(,)?) => {{
        $crate::__log_internal!($level, $msg $(, $key => $val)+);
    }};

    // Just message: info!("msg")
    ($level:expr, $msg:expr $(,)?) => {{
        $crate::__log_internal!($level, $msg);
    }};
}

/// Record a metric.
///
/// # Examples
///
/// ```text
/// // Counter (increments by 1)
/// metric!(counter "requests_total");
/// metric!(counter "requests_total", "method" => "GET");
///
/// // Counter with value (use second positional arg)
/// metric!(counter "bytes_sent", 1024);
/// metric!(counter "items_processed", count, "queue" => "jobs");
///
/// // Gauge (absolute value)
/// metric!(gauge "connections_active", pool.active());
/// metric!(gauge "queue_depth", len, "queue" => name);
///
/// // Histogram (record sample)
/// metric!(histogram "request_duration_ms", elapsed);
/// metric!(histogram "response_size", size, "endpoint" => path);
/// ```
#[macro_export]
macro_rules! metric {
    // Counter with no value (increment by 1), with optional tags
    (counter $name:expr $(, $key:expr => $val:expr)* $(,)?) => {{
        $crate::__metric_impl!(@counter $name, 1.0 $(, $key => $val)*);
    }};

    // Counter with value, with optional tags
    (counter $name:expr, $value:expr $(, $key:expr => $val:expr)* $(,)?) => {{
        $crate::__metric_impl!(@counter $name, $value as f64 $(, $key => $val)*);
    }};

    // Gauge with value, with optional tags
    (gauge $name:expr, $value:expr $(, $key:expr => $val:expr)* $(,)?) => {{
        $crate::__metric_impl!(@gauge $name, $value as f64 $(, $key => $val)*);
    }};

    // Histogram with value, with optional tags
    (histogram $name:expr, $value:expr $(, $key:expr => $val:expr)* $(,)?) => {{
        $crate::__metric_impl!(@histogram $name, $value as f64 $(, $key => $val)*);
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __metric_impl {
    (@counter $name:expr, $value:expr $(, $key:expr => $val:expr)* $(,)?) => {{
        if let Some(entry) = $crate::TelemetryContext::with(|ctx| {
            let mut common = $crate::CommonFields::new(&ctx.service, &ctx.node_id);
            common.trace_id = Some(ctx.trace_id.clone());
            common.span_id = Some(ctx.span_id.clone());
            $(
                common.tags.insert($key.into(), $val.to_string());
            )*
            $crate::MetricEntry::counter_with_value(common, $name, $value)
        }) {
            $crate::collect_metric(entry);
        }
    }};

    (@gauge $name:expr, $value:expr $(, $key:expr => $val:expr)* $(,)?) => {{
        if let Some(entry) = $crate::TelemetryContext::with(|ctx| {
            let mut common = $crate::CommonFields::new(&ctx.service, &ctx.node_id);
            common.trace_id = Some(ctx.trace_id.clone());
            common.span_id = Some(ctx.span_id.clone());
            $(
                common.tags.insert($key.into(), $val.to_string());
            )*
            $crate::MetricEntry::gauge(common, $name, $value)
        }) {
            $crate::collect_metric(entry);
        }
    }};

    (@histogram $name:expr, $value:expr $(, $key:expr => $val:expr)* $(,)?) => {{
        if let Some(entry) = $crate::TelemetryContext::with(|ctx| {
            let mut common = $crate::CommonFields::new(&ctx.service, &ctx.node_id);
            common.trace_id = Some(ctx.trace_id.clone());
            common.span_id = Some(ctx.span_id.clone());
            $(
                common.tags.insert($key.into(), $val.to_string());
            )*
            $crate::MetricEntry::histogram(common, $name, $value)
        }) {
            $crate::collect_metric(entry);
        }
    }};
}

/// Execute a block within a new child span.
///
/// The span is automatically ended when the block completes.
///
/// # Examples
///
/// ```text
/// // Basic span
/// let result = span!("db_query" => {
///     db.query(&sql).await
/// });
///
/// // With tags
/// let user = span!("fetch_user", "user_id" => id; {
///     users.find(id).await
/// });
/// ```
#[macro_export]
macro_rules! span {
    // Span with tags (semicolon separator before block)
    ($name:expr $(, $key:expr => $val:expr)* ; $block:expr) => {{
        $crate::__span_impl!($name, [$($key => $val),*], $block)
    }};

    // Span without tags (=> separator)
    ($name:expr => $block:expr) => {{
        $crate::__span_impl!($name, [], $block)
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __span_impl {
    ($name:expr, [$($key:expr => $val:expr),*], $block:expr) => {{
        (|| {
            let __span = match $crate::Span::new($name) {
                Some(s) => s,
                None => return $block, // No context, just run block
            };
            $(
                let __span = __span.tag($key, $val.to_string());
            )*
            let _guard = __span.enter();
            $block
        })()
    }};
}

#[cfg(test)]
mod tests {
    use crate::{
        drain, set_global_collector, BufferCollector, CollectorConfig, Level, MetricType,
        TelemetryContext, TelemetryEntry,
    };
    use std::sync::Arc;

    // Single comprehensive test to avoid global collector conflicts
    // Global collector can only be set once, so parallel tests interfere
    #[tokio::test]
    async fn macro_integration_test() {
        // Set up collector once
        let _ = set_global_collector(Arc::new(BufferCollector::new(CollectorConfig::new(1000))));

        // Helper to drain and filter entries by service (other tests may contribute entries)
        fn drain_for_service(service: &str) -> Vec<TelemetryEntry> {
            drain()
                .into_iter()
                .filter(|e| e.service() == service)
                .collect()
        }

        let _ = drain(); // Clear any existing entries

        // Test 1: Log macros basic
        {
            let ctx = TelemetryContext::new("macro-test-1", "node-1");
            ctx.scope(async {
                info!("Hello world");
                warn!("Warning message");
                error!("Error occurred");
                debug!("Debug info");
            })
            .await;

            let entries = drain_for_service("macro-test-1");
            assert_eq!(entries.len(), 4, "log_macros_basic");

            let levels: Vec<_> = entries
                .iter()
                .filter_map(|e| match e {
                    TelemetryEntry::Log(l) => Some(l.level),
                    _ => None,
                })
                .collect();
            assert_eq!(levels, vec![Level::Info, Level::Warn, Level::Error, Level::Debug]);
        }

        // Test 2: Log macros with tags
        {
            let ctx = TelemetryContext::new("macro-test-2", "node-1");
            ctx.scope(async {
                info!("Request handled", "method" => "GET", "path" => "/api");
            })
            .await;

            let entries = drain_for_service("macro-test-2");
            assert_eq!(entries.len(), 1, "log_macros_with_tags");

            if let TelemetryEntry::Log(log) = &entries[0] {
                assert_eq!(log.message, "Request handled");
                assert_eq!(log.common.tags.get("method"), Some(&"GET".to_string()));
                assert_eq!(log.common.tags.get("path"), Some(&"/api".to_string()));
            }
        }

        // Test 3: Log macros with format
        {
            let ctx = TelemetryContext::new("macro-test-3", "node-1");
            ctx.scope(async {
                let user_id = 42;
                info!("User {} logged in", user_id);
            })
            .await;

            let entries = drain_for_service("macro-test-3");
            assert_eq!(entries.len(), 1, "log_macros_with_format");

            if let TelemetryEntry::Log(log) = &entries[0] {
                assert_eq!(log.message, "User 42 logged in");
            }
        }

        // Test 4: Source location capture
        {
            let ctx = TelemetryContext::new("macro-test-4", "node-1");
            ctx.scope(async {
                info!("Test message");
            })
            .await;

            let entries = drain_for_service("macro-test-4");
            if let TelemetryEntry::Log(log) = &entries[0] {
                assert!(log.file.is_some(), "file should be captured");
                assert!(log.line.is_some(), "line should be captured");
                assert!(log.file.as_ref().unwrap().contains("macros.rs"));
            }
        }

        // Test 5: Metric counters
        {
            let ctx = TelemetryContext::new("macro-test-5", "node-1");
            ctx.scope(async {
                metric!(counter "requests_total");
                metric!(counter "bytes_sent", 1024);
                metric!(counter "items", 5, "queue" => "jobs");
            })
            .await;

            let entries = drain_for_service("macro-test-5");
            assert_eq!(entries.len(), 3, "metric_counter");

            if let TelemetryEntry::Metric(m) = &entries[0] {
                assert_eq!(m.name, "requests_total");
                assert_eq!(m.metric_type, MetricType::Counter);
                assert_eq!(m.value, 1.0);
            }
            if let TelemetryEntry::Metric(m) = &entries[1] {
                assert_eq!(m.name, "bytes_sent");
                assert_eq!(m.value, 1024.0);
            }
            if let TelemetryEntry::Metric(m) = &entries[2] {
                assert_eq!(m.name, "items");
                assert_eq!(m.value, 5.0);
                assert_eq!(m.common.tags.get("queue"), Some(&"jobs".to_string()));
            }
        }

        // Test 6: Metric gauge
        {
            let ctx = TelemetryContext::new("macro-test-6", "node-1");
            ctx.scope(async {
                metric!(gauge "connections", 42);
                metric!(gauge "memory_mb", 512.5, "host" => "server1");
            })
            .await;

            let entries = drain_for_service("macro-test-6");
            assert_eq!(entries.len(), 2, "metric_gauge");

            if let TelemetryEntry::Metric(m) = &entries[0] {
                assert_eq!(m.name, "connections");
                assert_eq!(m.metric_type, MetricType::Gauge);
                assert_eq!(m.value, 42.0);
            }
        }

        // Test 7: Metric histogram
        {
            let ctx = TelemetryContext::new("macro-test-7", "node-1");
            ctx.scope(async {
                metric!(histogram "latency_ms", 150);
                metric!(histogram "size_bytes", 2048, "endpoint" => "/api");
            })
            .await;

            let entries = drain_for_service("macro-test-7");
            assert_eq!(entries.len(), 2, "metric_histogram");

            if let TelemetryEntry::Metric(m) = &entries[0] {
                assert_eq!(m.name, "latency_ms");
                assert_eq!(m.metric_type, MetricType::Histogram);
                assert_eq!(m.value, 150.0);
            }
        }

        // Test 8: Span macro basic
        {
            let ctx = TelemetryContext::new("macro-test-8", "node-1");
            ctx.scope(async {
                let result = span!("compute" => {
                    42
                });
                assert_eq!(result, 42);
            })
            .await;

            let entries = drain_for_service("macro-test-8");
            assert_eq!(entries.len(), 1, "span_macro_basic");

            if let TelemetryEntry::Span(s) = &entries[0] {
                assert_eq!(s.name, "compute");
            }
        }

        // Test 9: Span macro with tags
        {
            let ctx = TelemetryContext::new("macro-test-9", "node-1");
            ctx.scope(async {
                let user_id = "user-123";
                span!("fetch_user", "user_id" => user_id; {
                    // fetch user
                });
            })
            .await;

            let entries = drain_for_service("macro-test-9");
            assert_eq!(entries.len(), 1, "span_macro_with_tags");

            if let TelemetryEntry::Span(s) = &entries[0] {
                assert_eq!(s.name, "fetch_user");
                assert_eq!(s.common.tags.get("user_id"), Some(&"user-123".to_string()));
            }
        }

        // Test 10: No context - no panic, no collection
        {
            let _ = drain(); // Clear first

            // Outside any context
            info!("No context");
            metric!(counter "orphan");

            let result = span!("orphan" => {
                42
            });
            assert_eq!(result, 42);

            // Should not have added any entries (no context = no service to filter by, but also nothing added)
            let all_entries = drain();
            let orphan_entries: Vec<_> = all_entries.iter().filter(|e| e.service().is_empty()).collect();
            assert!(orphan_entries.is_empty(), "no_context_no_collection");
        }

        // Test 11: Counter with tags only (no value)
        {
            let ctx = TelemetryContext::new("macro-test-11", "node-1");
            ctx.scope(async {
                metric!(counter "tagged", "env" => "prod");
            })
            .await;

            let entries = drain_for_service("macro-test-11");
            assert_eq!(entries.len(), 1, "counter_with_tags_only");

            if let TelemetryEntry::Metric(m) = &entries[0] {
                assert_eq!(m.name, "tagged");
                assert_eq!(m.value, 1.0);
                assert_eq!(m.common.tags.get("env"), Some(&"prod".to_string()));
            }
        }
    }
}
