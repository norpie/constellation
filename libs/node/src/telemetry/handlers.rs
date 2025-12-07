//! Telemetry handlers for scraping collected telemetry data.

use crate::handler;
use crate::Data;
use constellation_telemetry::{BufferCollector, Collector, TelemetryEntry};
use serde::{Deserialize, Serialize};

/// Request for scraping telemetry (empty - just triggers drain)
#[derive(Debug, Serialize, Deserialize)]
pub struct ScrapeRequest {}

/// Response containing collected telemetry entries
#[derive(Debug, Serialize, Deserialize)]
pub struct ScrapeResponse {
    /// The collected telemetry entries
    pub entries: Vec<TelemetryEntry>,
}

/// Error type for scrape handler
#[derive(Debug, Serialize, Deserialize)]
pub struct ScrapeError(pub String);

impl std::fmt::Display for ScrapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ScrapeError {}

impl crate::rpc::ErrorResponder for ScrapeError {
    fn error_category(&self) -> crate::rpc::ErrorCategory {
        crate::rpc::ErrorCategory::ServerError
    }
}

/// Handler for `_telemetry.scrape`
///
/// Drains the collector buffer and returns all collected telemetry entries.
/// This is called by the telemetry service to collect telemetry from nodes.
#[handler(route = "_telemetry.scrape")]
async fn telemetry_scrape(
    _req: ScrapeRequest,
    collector: Data<BufferCollector>,
) -> Result<ScrapeResponse, ScrapeError> {
    let entries = collector.drain();
    Ok(ScrapeResponse { entries })
}
