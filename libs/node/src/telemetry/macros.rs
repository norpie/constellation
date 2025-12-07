//! Telemetry macros for logging, metrics, and spans.
//!
//! These macros automatically capture context from task-local storage
//! and emit telemetry entries to the collector.

/// Internal macro for creating log entries.
///
/// Not intended for direct use - use `error!`, `warn!`, `info!`, or `debug!` instead.
#[macro_export]
#[doc(hidden)]
macro_rules! __telemetry_log {
    ($level:expr, $msg:expr) => {{
        $crate::telemetry::TelemetryContext::with(|ctx| {
            use $crate::telemetry::Collector;

            let common = $crate::telemetry::CommonFields::new(&ctx.service, &ctx.node_id)
                .with_trace_id(ctx.trace_id.clone())
                .with_span_id(ctx.span_id.clone());

            let entry = $crate::telemetry::LogEntry::new(common, $level, $msg)
                .with_location(module_path!(), file!(), line!());

            ctx.collector.collect_log(entry);
        });
    }};
    ($level:expr, $msg:expr, $($key:ident = $value:expr),+ $(,)?) => {{
        $crate::telemetry::TelemetryContext::with(|ctx| {
            use $crate::telemetry::Collector;

            let common = $crate::telemetry::CommonFields::new(&ctx.service, &ctx.node_id)
                .with_trace_id(ctx.trace_id.clone())
                .with_span_id(ctx.span_id.clone())
                $(
                    .with_tag(stringify!($key), $value.to_string())
                )+;

            let entry = $crate::telemetry::LogEntry::new(common, $level, $msg)
                .with_location(module_path!(), file!(), line!());

            ctx.collector.collect_log(entry);
        });
    }};
}

/// Log an error message.
///
/// # Example
///
/// ```ignore
/// error!("Failed to connect to database");
/// error!("User not found", user_id = user_id);
/// ```
#[macro_export]
macro_rules! error {
    ($msg:expr $(, $key:ident = $value:expr)*) => {
        $crate::__telemetry_log!($crate::telemetry::Level::Error, $msg $(, $key = $value)*)
    };
}

/// Log a warning message.
///
/// # Example
///
/// ```ignore
/// warn!("Rate limit approaching");
/// warn!("Slow query detected", duration_ms = elapsed);
/// ```
#[macro_export]
macro_rules! warn {
    ($msg:expr $(, $key:ident = $value:expr)*) => {
        $crate::__telemetry_log!($crate::telemetry::Level::Warn, $msg $(, $key = $value)*)
    };
}

/// Log an info message.
///
/// # Example
///
/// ```ignore
/// info!("Server started");
/// info!("Request processed", status = 200);
/// ```
#[macro_export]
macro_rules! info {
    ($msg:expr $(, $key:ident = $value:expr)*) => {
        $crate::__telemetry_log!($crate::telemetry::Level::Info, $msg $(, $key = $value)*)
    };
}

/// Log a debug message.
///
/// # Example
///
/// ```ignore
/// debug!("Entering function");
/// debug!("Cache lookup", key = cache_key);
/// ```
#[macro_export]
macro_rules! debug {
    ($msg:expr $(, $key:ident = $value:expr)*) => {
        $crate::__telemetry_log!($crate::telemetry::Level::Debug, $msg $(, $key = $value)*)
    };
}

/// Record a metric.
///
/// # Variants
///
/// - `metric!(counter "name")` - Increment a counter by 1
/// - `metric!(counter "name", value)` - Increment a counter by value
/// - `metric!(gauge "name", value)` - Set a gauge to value
/// - `metric!(histogram "name", value)` - Record a histogram observation
///
/// # Example
///
/// ```ignore
/// metric!(counter "requests_total");
/// metric!(counter "bytes_sent", bytes.len());
/// metric!(gauge "connections_active", active_count);
/// metric!(histogram "request_duration_ms", elapsed.as_millis());
/// ```
#[macro_export]
macro_rules! metric {
    (counter $name:expr) => {
        $crate::metric!(counter $name, 1.0)
    };
    (counter $name:expr, $value:expr) => {{
        $crate::telemetry::TelemetryContext::with(|ctx| {
            use $crate::telemetry::Collector;

            let common = $crate::telemetry::CommonFields::new(&ctx.service, &ctx.node_id)
                .with_trace_id(ctx.trace_id.clone())
                .with_span_id(ctx.span_id.clone());

            let entry = $crate::telemetry::MetricEntry::counter_with_value(common, $name, $value as f64);

            ctx.collector.collect_metric(entry);
        });
    }};
    (gauge $name:expr, $value:expr) => {{
        $crate::telemetry::TelemetryContext::with(|ctx| {
            use $crate::telemetry::Collector;

            let common = $crate::telemetry::CommonFields::new(&ctx.service, &ctx.node_id)
                .with_trace_id(ctx.trace_id.clone())
                .with_span_id(ctx.span_id.clone());

            let entry = $crate::telemetry::MetricEntry::gauge(common, $name, $value as f64);

            ctx.collector.collect_metric(entry);
        });
    }};
    (histogram $name:expr, $value:expr) => {{
        $crate::telemetry::TelemetryContext::with(|ctx| {
            use $crate::telemetry::Collector;

            let common = $crate::telemetry::CommonFields::new(&ctx.service, &ctx.node_id)
                .with_trace_id(ctx.trace_id.clone())
                .with_span_id(ctx.span_id.clone());

            let entry = $crate::telemetry::MetricEntry::histogram(common, $name, $value as f64);

            ctx.collector.collect_metric(entry);
        });
    }};
}

/// Create a span for tracing an operation.
///
/// # Variants
///
/// - `span!("name")` - Create a span guard (drop to end)
/// - `span!("name", { ... })` - Create a span that wraps a block
///
/// # Example
///
/// ```ignore
/// // Guard style - span ends when guard is dropped
/// let _span = span!("database_query");
/// let result = db.query(&sql).await?;
///
/// // Block style - span wraps the block
/// let result = span!("process_request", {
///     validate_input(&req)?;
///     process(&req).await
/// });
/// ```
#[macro_export]
macro_rules! span {
    ($name:expr) => {
        $crate::telemetry::Span::enter($name)
    };
    ($name:expr, $body:block) => {{
        let _guard = $crate::telemetry::Span::enter($name);
        $body
    }};
}

#[cfg(test)]
mod tests {
    use crate::telemetry::{Collector, TelemetryContext};
    use crate::Data;
    use constellation_telemetry::{BufferCollector, CollectorConfig, Level, TelemetryEntry};

    fn make_collector() -> Data<BufferCollector> {
        Data::new(BufferCollector::new(CollectorConfig::default()))
    }

    #[tokio::test]
    async fn log_macros() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            error!("error message");
            warn!("warn message");
            info!("info message");
            debug!("debug message");
        })
        .await;

        let entries = collector.drain();
        assert_eq!(entries.len(), 4);

        let levels: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                TelemetryEntry::Log(l) => Some(l.level),
                _ => None,
            })
            .collect();
        assert_eq!(levels, vec![Level::Error, Level::Warn, Level::Info, Level::Debug]);
    }

    #[tokio::test]
    async fn log_with_tags() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            info!("user action", user_id = "123", action = "login");
        })
        .await;

        let entries = collector.drain();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            TelemetryEntry::Log(log) => {
                assert_eq!(log.common.tags.get("user_id"), Some(&"123".to_string()));
                assert_eq!(log.common.tags.get("action"), Some(&"login".to_string()));
            }
            _ => panic!("Expected log entry"),
        }
    }

    #[tokio::test]
    async fn log_captures_location() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            info!("test message");
        })
        .await;

        let entries = collector.drain();
        match &entries[0] {
            TelemetryEntry::Log(log) => {
                assert!(log.file.is_some());
                assert!(log.line.is_some());
                assert!(log.target.is_some());
            }
            _ => panic!("Expected log entry"),
        }
    }

    #[tokio::test]
    async fn metric_counter() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            metric!(counter "requests_total");
            metric!(counter "bytes_sent", 1024);
        })
        .await;

        let entries = collector.drain();
        assert_eq!(entries.len(), 2);

        let metrics: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                TelemetryEntry::Metric(m) => Some((m.name.as_str(), m.value)),
                _ => None,
            })
            .collect();

        assert_eq!(metrics[0], ("requests_total", 1.0));
        assert_eq!(metrics[1], ("bytes_sent", 1024.0));
    }

    #[tokio::test]
    async fn metric_gauge() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            metric!(gauge "connections_active", 42);
        })
        .await;

        let entries = collector.drain();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            TelemetryEntry::Metric(m) => {
                assert_eq!(m.name, "connections_active");
                assert_eq!(m.value, 42.0);
            }
            _ => panic!("Expected metric entry"),
        }
    }

    #[tokio::test]
    async fn metric_histogram() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            metric!(histogram "request_duration_ms", 150.5);
        })
        .await;

        let entries = collector.drain();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            TelemetryEntry::Metric(m) => {
                assert_eq!(m.name, "request_duration_ms");
                assert_eq!(m.value, 150.5);
            }
            _ => panic!("Expected metric entry"),
        }
    }

    #[tokio::test]
    async fn span_guard_style() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        ctx.scope(async {
            let _guard = span!("my_operation");
            // Do some work...
        })
        .await;

        let entries = collector.drain();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            TelemetryEntry::Span(s) => {
                assert_eq!(s.name, "my_operation");
            }
            _ => panic!("Expected span entry"),
        }
    }

    #[tokio::test]
    async fn span_block_style() {
        let collector = make_collector();
        let ctx = TelemetryContext::new("TestService", "test-1", collector.clone());

        let result = ctx
            .scope(async {
                span!("computation", {
                    1 + 2
                })
            })
            .await;

        assert_eq!(result, 3);

        let entries = collector.drain();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            TelemetryEntry::Span(s) => {
                assert_eq!(s.name, "computation");
            }
            _ => panic!("Expected span entry"),
        }
    }

    #[tokio::test]
    async fn macros_outside_context() {
        // These should not panic when called outside a context
        info!("orphan log");
        metric!(counter "orphan_metric");
        let _guard = span!("orphan_span");
    }
}
