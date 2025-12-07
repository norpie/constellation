//! Built-in handlers for health checks

use crate::health::{
    HealthRegistry, HealthStatus, ReadyRequest, ReadyResponse, StatusRequest, StatusResponse,
};
use crate::handler;
use crate::metric;
use crate::Data;
use std::time::Instant;

/// Get detailed health status
///
/// Returns the overall health status, startup state, and individual check results.
/// Emits `health_status` gauge (2=healthy, 1=degraded, 0=unhealthy)
/// and `health_check_duration_ms` histogram when telemetry is enabled.
#[handler(route = "_health.status")]
async fn health_status(
    _req: StatusRequest,
    registry: Data<HealthRegistry>,
) -> Result<StatusResponse, crate::error::Error> {
    let start = Instant::now();
    let response = registry.check_status().await;
    let duration_ms = start.elapsed().as_millis();

    // Emit metrics (no-op if not in telemetry context)
    let status_value = match response.status {
        HealthStatus::Starting => 3.0,
        HealthStatus::Healthy => 2.0,
        HealthStatus::Degraded => 1.0,
        HealthStatus::Unhealthy => 0.0,
    };
    metric!(gauge "health_status", status_value);
    metric!(histogram "health_check_duration_ms", duration_ms);

    Ok(response)
}

/// Check if the node is ready
///
/// Returns true only if all health checks pass and the node has completed startup.
/// Emits `health_ready` gauge (1=ready, 0=not ready)
/// and `health_ready_duration_ms` histogram when telemetry is enabled.
#[handler(route = "_health.ready")]
async fn health_ready(
    _req: ReadyRequest,
    registry: Data<HealthRegistry>,
) -> Result<ReadyResponse, crate::error::Error> {
    let start = Instant::now();
    let ready = registry.is_ready().await;
    let duration_ms = start.elapsed().as_millis();

    // Emit metrics (no-op if not in telemetry context)
    metric!(gauge "health_ready", if ready { 1.0 } else { 0.0 });
    metric!(histogram "health_ready_duration_ms", duration_ms);

    Ok(ReadyResponse { ready })
}
