//! Histogram aggregation and rollups.
//!
//! This module handles aggregating raw histogram samples into time-bucketed
//! rollups with summary statistics (count, sum, min, max, percentiles).

use crate::error::Result;
use crate::key::{PrimaryKey, PRIMARY_KEY_SIZE};
use crate::store::Datapad;
use constellation_telemetry::{EntryType, MetricType, Timestamp, TelemetryEntry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Duration of aggregation buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RollupInterval {
    /// 1 minute buckets
    OneMinute,
    /// 1 hour buckets
    OneHour,
    /// 1 day buckets
    OneDay,
}

impl RollupInterval {
    /// Get the interval duration in microseconds.
    pub fn as_micros(&self) -> u64 {
        match self {
            RollupInterval::OneMinute => 60 * 1_000_000,
            RollupInterval::OneHour => 60 * 60 * 1_000_000,
            RollupInterval::OneDay => 24 * 60 * 60 * 1_000_000,
        }
    }

    /// Get the tree name for this rollup interval.
    pub fn tree_name(&self) -> &'static str {
        match self {
            RollupInterval::OneMinute => "rollup_1m",
            RollupInterval::OneHour => "rollup_1h",
            RollupInterval::OneDay => "rollup_1d",
        }
    }

    /// Round a timestamp down to the start of a bucket.
    pub fn bucket_start(&self, timestamp: Timestamp) -> Timestamp {
        let interval = self.as_micros();
        (timestamp / interval) * interval
    }
}

/// Aggregated statistics for a time bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupEntry {
    /// Metric name
    pub name: String,

    /// Service name
    pub service: String,

    /// Start of the time bucket
    pub bucket_start: Timestamp,

    /// Rollup interval
    pub interval_micros: u64,

    /// Number of samples
    pub count: u64,

    /// Sum of all values
    pub sum: f64,

    /// Minimum value
    pub min: f64,

    /// Maximum value
    pub max: f64,

    /// 50th percentile (median)
    pub p50: f64,

    /// 90th percentile
    pub p90: f64,

    /// 99th percentile
    pub p99: f64,
}

impl RollupEntry {
    /// Create a new rollup from samples.
    pub fn from_samples(
        name: String,
        service: String,
        bucket_start: Timestamp,
        interval: RollupInterval,
        mut samples: Vec<f64>,
    ) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }

        // Sort for percentile calculation
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let count = samples.len() as u64;
        let sum: f64 = samples.iter().sum();
        let min = samples.first().copied().unwrap_or(0.0);
        let max = samples.last().copied().unwrap_or(0.0);

        Some(Self {
            name,
            service,
            bucket_start,
            interval_micros: interval.as_micros(),
            count,
            sum,
            min,
            max,
            p50: percentile(&samples, 50.0),
            p90: percentile(&samples, 90.0),
            p99: percentile(&samples, 99.0),
        })
    }

    /// Merge another rollup into this one.
    ///
    /// Note: Percentiles are approximated using weighted averages.
    pub fn merge(&mut self, other: &RollupEntry) {
        let total_count = self.count + other.count;
        if total_count == 0 {
            return;
        }

        let self_weight = self.count as f64 / total_count as f64;
        let other_weight = other.count as f64 / total_count as f64;

        self.sum += other.sum;
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);

        // Approximate percentiles using weighted average
        self.p50 = self.p50 * self_weight + other.p50 * other_weight;
        self.p90 = self.p90 * self_weight + other.p90 * other_weight;
        self.p99 = self.p99 * self_weight + other.p99 * other_weight;

        self.count = total_count;
    }

    /// Get the mean value.
    pub fn mean(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / self.count as f64
        }
    }
}

/// Calculate a percentile from sorted samples.
fn percentile(sorted_samples: &[f64], p: f64) -> f64 {
    if sorted_samples.is_empty() {
        return 0.0;
    }
    if sorted_samples.len() == 1 {
        return sorted_samples[0];
    }

    let idx = (p / 100.0 * (sorted_samples.len() - 1) as f64).round() as usize;
    sorted_samples[idx.min(sorted_samples.len() - 1)]
}

/// Key for a rollup entry: [bucket_start: u64][service_len: u16][service][metric_name]
fn rollup_key(bucket_start: Timestamp, service: &str, name: &str) -> Vec<u8> {
    let mut key = Vec::new();
    key.extend_from_slice(&bucket_start.to_be_bytes());
    key.extend_from_slice(&(service.len() as u16).to_be_bytes());
    key.extend_from_slice(service.as_bytes());
    key.extend_from_slice(name.as_bytes());
    key
}

impl Datapad {
    /// Aggregate histogram metrics for a time range into rollups.
    ///
    /// This reads raw histogram entries, groups them by (metric_name, service, bucket),
    /// and stores aggregated rollups.
    pub fn aggregate_histograms(
        &self,
        start: Timestamp,
        end: Timestamp,
        interval: RollupInterval,
    ) -> Result<usize> {
        // Collect histogram samples by (service, metric_name, bucket_start)
        let mut buckets: HashMap<(String, String, Timestamp), Vec<f64>> = HashMap::new();

        // Query histogram metrics in the time range
        let entries = self.db().open_tree("entries")?;
        let start_key = PrimaryKey::range_start(start);
        let end_key = PrimaryKey::range_end(end);

        for item in entries.range(start_key..=end_key) {
            let (key, value) = item?;
            if key.len() != PRIMARY_KEY_SIZE {
                continue;
            }

            let pk = match PrimaryKey::decode(&key) {
                Some(pk) if pk.entry_type == EntryType::Metric => pk,
                _ => continue,
            };

            let entry: TelemetryEntry = bincode::deserialize(&value)
                .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

            if let TelemetryEntry::Metric(metric) = entry {
                if metric.metric_type != MetricType::Histogram {
                    continue;
                }

                let bucket_start = interval.bucket_start(pk.timestamp);
                let key = (
                    metric.common.service.clone(),
                    metric.name.clone(),
                    bucket_start,
                );

                let samples = buckets.entry(key).or_default();
                if let Some(histogram) = &metric.histogram {
                    samples.extend(histogram.iter().copied());
                } else {
                    samples.push(metric.value);
                }
            }
        }

        // Create and store rollups
        let rollup_tree = self.db().open_tree(interval.tree_name())?;
        let mut count = 0;

        for ((service, name, bucket_start), samples) in buckets {
            if let Some(rollup) = RollupEntry::from_samples(
                name.clone(),
                service.clone(),
                bucket_start,
                interval,
                samples,
            ) {
                let key = rollup_key(bucket_start, &service, &name);
                let value = bincode::serialize(&rollup)
                    .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

                // Merge with existing rollup if present
                if let Some(existing) = rollup_tree.get(&key)? {
                    let mut existing_rollup: RollupEntry = bincode::deserialize(&existing)
                        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;
                    existing_rollup.merge(&rollup);
                    let merged_value = bincode::serialize(&existing_rollup)
                        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;
                    rollup_tree.insert(&key, merged_value)?;
                } else {
                    rollup_tree.insert(&key, value)?;
                }

                count += 1;
            }
        }

        Ok(count)
    }

    /// Query rollups for a metric.
    pub fn query_rollups(
        &self,
        name: &str,
        service: Option<&str>,
        start: Timestamp,
        end: Timestamp,
        interval: RollupInterval,
    ) -> Result<Vec<RollupEntry>> {
        let rollup_tree = self.db().open_tree(interval.tree_name())?;
        let mut results = Vec::new();

        // Scan through the rollup tree
        let start_bucket = interval.bucket_start(start);
        let end_bucket = interval.bucket_start(end);

        for item in rollup_tree.iter() {
            let (key, value) = item?;

            // Decode bucket_start from key
            if key.len() < 8 {
                continue;
            }
            let bucket_start = u64::from_be_bytes(key[0..8].try_into().unwrap());

            if bucket_start < start_bucket || bucket_start > end_bucket {
                continue;
            }

            let rollup: RollupEntry = bincode::deserialize(&value)
                .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

            // Filter by name
            if rollup.name != name {
                continue;
            }

            // Filter by service if specified
            if let Some(svc) = service {
                if rollup.service != svc {
                    continue;
                }
            }

            results.push(rollup);
        }

        // Sort by bucket_start
        results.sort_by_key(|r| r.bucket_start);

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use constellation_telemetry::{CommonFields, MetricEntry};

    fn make_histogram_at(timestamp: Timestamp, service: &str, name: &str, samples: Vec<f64>) -> TelemetryEntry {
        let mut common = CommonFields::new(service, "node-1");
        common.timestamp = timestamp;
        MetricEntry::histogram_batch(common, name, samples).into()
    }

    #[test]
    fn rollup_from_samples() {
        let samples = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let rollup = RollupEntry::from_samples(
            "latency".to_string(),
            "api".to_string(),
            1000,
            RollupInterval::OneMinute,
            samples,
        )
        .unwrap();

        assert_eq!(rollup.count, 5);
        assert_eq!(rollup.sum, 15.0);
        assert_eq!(rollup.min, 1.0);
        assert_eq!(rollup.max, 5.0);
        assert_eq!(rollup.mean(), 3.0);
    }

    #[test]
    fn rollup_percentiles() {
        // 100 samples from 1 to 100
        let samples: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        let rollup = RollupEntry::from_samples(
            "latency".to_string(),
            "api".to_string(),
            1000,
            RollupInterval::OneMinute,
            samples,
        )
        .unwrap();

        // Percentiles use nearest-rank method
        assert!((rollup.p50 - 50.0).abs() <= 1.0);
        assert!((rollup.p90 - 90.0).abs() <= 1.0);
        assert!((rollup.p99 - 99.0).abs() <= 1.0);
    }

    #[test]
    fn rollup_merge() {
        let samples1 = vec![1.0, 2.0, 3.0];
        let mut rollup1 = RollupEntry::from_samples(
            "latency".to_string(),
            "api".to_string(),
            1000,
            RollupInterval::OneMinute,
            samples1,
        )
        .unwrap();

        let samples2 = vec![4.0, 5.0, 6.0];
        let rollup2 = RollupEntry::from_samples(
            "latency".to_string(),
            "api".to_string(),
            1000,
            RollupInterval::OneMinute,
            samples2,
        )
        .unwrap();

        rollup1.merge(&rollup2);

        assert_eq!(rollup1.count, 6);
        assert_eq!(rollup1.sum, 21.0);
        assert_eq!(rollup1.min, 1.0);
        assert_eq!(rollup1.max, 6.0);
    }

    #[test]
    fn bucket_start_calculation() {
        let timestamp = 1_000_000 * (60 * 5 + 30); // 5 minutes 30 seconds in micros

        assert_eq!(
            RollupInterval::OneMinute.bucket_start(timestamp),
            1_000_000 * 60 * 5 // 5 minutes
        );

        assert_eq!(
            RollupInterval::OneHour.bucket_start(timestamp),
            0 // 0 hours (first hour)
        );
    }

    #[test]
    fn aggregate_and_query() {
        let datapad = Datapad::open_temporary().unwrap();

        let now = 60 * 60 * 1_000_000u64; // 1 hour in micros (simple timestamp)
        let minute = 60 * 1_000_000u64;

        // Insert histogram samples at different times in the same minute bucket
        datapad
            .insert(&make_histogram_at(now, "api", "latency", vec![10.0, 20.0]))
            .unwrap();
        datapad
            .insert(&make_histogram_at(now + 10_000_000, "api", "latency", vec![15.0, 25.0]))
            .unwrap();
        datapad
            .insert(&make_histogram_at(now + 20_000_000, "api", "latency", vec![30.0]))
            .unwrap();

        // Different metric
        datapad
            .insert(&make_histogram_at(now, "api", "errors", vec![1.0]))
            .unwrap();

        // Aggregate
        let count = datapad
            .aggregate_histograms(now, now + minute, RollupInterval::OneMinute)
            .unwrap();
        assert_eq!(count, 2); // Two different metrics

        // Query latency rollups
        let rollups = datapad
            .query_rollups("latency", Some("api"), now, now + minute, RollupInterval::OneMinute)
            .unwrap();
        assert_eq!(rollups.len(), 1);

        let rollup = &rollups[0];
        assert_eq!(rollup.count, 5); // 2 + 2 + 1 samples
        assert_eq!(rollup.sum, 100.0); // 10+20+15+25+30
        assert_eq!(rollup.min, 10.0);
        assert_eq!(rollup.max, 30.0);
    }
}
