use serde::{Deserialize, Serialize};

use super::common::CommonFields;

/// Type of metric
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricType {
    /// Monotonically increasing counter (e.g., request count)
    Counter,
    /// Point-in-time value (e.g., active connections)
    Gauge,
    /// Distribution of values (e.g., request latency)
    Histogram,
}

impl MetricType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricType::Counter => "counter",
            MetricType::Gauge => "gauge",
            MetricType::Histogram => "histogram",
        }
    }
}

impl std::fmt::Display for MetricType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A metric entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricEntry {
    /// Common fields (id, timestamp, service, node_id, trace_id, span_id, tags)
    pub common: CommonFields,

    /// Metric name (e.g., "requests_total", "request_duration_ms")
    pub name: String,

    /// Type of metric
    pub metric_type: MetricType,

    /// Current value (for counter/gauge)
    pub value: f64,

    /// Raw histogram samples (for histogram type)
    /// Stored as raw values, aggregated into rollups later
    pub histogram: Option<Vec<f64>>,
}

impl MetricEntry {
    /// Create a counter metric (increments by 1)
    pub fn counter(common: CommonFields, name: impl Into<String>) -> Self {
        Self {
            common,
            name: name.into(),
            metric_type: MetricType::Counter,
            value: 1.0,
            histogram: None,
        }
    }

    /// Create a counter metric with specific increment
    pub fn counter_with_value(common: CommonFields, name: impl Into<String>, value: f64) -> Self {
        Self {
            common,
            name: name.into(),
            metric_type: MetricType::Counter,
            value,
            histogram: None,
        }
    }

    /// Create a gauge metric
    pub fn gauge(common: CommonFields, name: impl Into<String>, value: f64) -> Self {
        Self {
            common,
            name: name.into(),
            metric_type: MetricType::Gauge,
            value,
            histogram: None,
        }
    }

    /// Create a histogram metric with a single sample
    pub fn histogram(common: CommonFields, name: impl Into<String>, sample: f64) -> Self {
        Self {
            common,
            name: name.into(),
            metric_type: MetricType::Histogram,
            value: sample, // Primary value is the sample
            histogram: Some(vec![sample]),
        }
    }

    /// Create a histogram metric with multiple samples
    pub fn histogram_batch(
        common: CommonFields,
        name: impl Into<String>,
        samples: Vec<f64>,
    ) -> Self {
        let value = samples.first().copied().unwrap_or(0.0);
        Self {
            common,
            name: name.into(),
            metric_type: MetricType::Histogram,
            value,
            histogram: Some(samples),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_creation() {
        let common = CommonFields::new("api", "api-1");
        let metric = MetricEntry::counter(common, "requests_total");

        assert_eq!(metric.name, "requests_total");
        assert_eq!(metric.metric_type, MetricType::Counter);
        assert_eq!(metric.value, 1.0);
        assert!(metric.histogram.is_none());
    }

    #[test]
    fn gauge_creation() {
        let common = CommonFields::new("api", "api-1");
        let metric = MetricEntry::gauge(common, "active_connections", 42.0);

        assert_eq!(metric.metric_type, MetricType::Gauge);
        assert_eq!(metric.value, 42.0);
    }

    #[test]
    fn histogram_creation() {
        let common = CommonFields::new("api", "api-1");
        let metric = MetricEntry::histogram(common, "request_duration_ms", 150.5);

        assert_eq!(metric.metric_type, MetricType::Histogram);
        assert_eq!(metric.histogram, Some(vec![150.5]));
    }
}
