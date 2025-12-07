//! Health check system for the node framework
//!
//! Provides two endpoints:
//! - `_health.status`: Returns detailed health status including startup state and check results
//! - `_health.ready`: Returns a simple boolean readiness check
//!
//! # Example
//! ```text
//! Node::builder()
//!     .service_name("MyService")
//!     .health_check("database", || async {
//!         // Check database connection
//!         Ok(())
//!     })
//!     .health_check("cache", || async {
//!         // Check cache connection
//!         Ok(())
//!     })
//!     .build()
//! ```

mod handlers;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Overall health status of the node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum HealthStatus {
    /// Node is still starting up
    Starting,
    /// All checks passing
    Healthy,
    /// Some checks failing, but node is operational
    Degraded,
    /// Critical checks failing, node is not operational
    Unhealthy,
}

/// Result of an individual health check
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CheckResult {
    /// Whether the check passed
    pub ok: bool,
    /// Optional error message if check failed
    pub error: Option<String>,
}

impl CheckResult {
    /// Create a passing check result
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }

    /// Create a failing check result
    pub fn fail(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(error.into()),
        }
    }
}

/// Request for health status endpoint
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusRequest {}

/// Response from health status endpoint
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatusResponse {
    /// Overall health status
    pub status: HealthStatus,
    /// Individual check results by name
    pub checks: HashMap<String, CheckResult>,
}

/// Request for readiness endpoint
#[derive(Debug, Serialize, Deserialize)]
pub struct ReadyRequest {}

/// Response from readiness endpoint
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadyResponse {
    /// Whether the node is ready to accept traffic
    pub ready: bool,
}

/// Type alias for async health check functions
pub type CheckFn =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> + Send + Sync>;

/// A named health check
pub struct HealthCheck {
    /// Name of the check
    pub name: String,
    /// The check function
    pub check: CheckFn,
}

/// Registry of health checks
///
/// Stored as `Data<HealthRegistry>` and accessed by health handlers.
pub struct HealthRegistry {
    checks: Vec<HealthCheck>,
    started: AtomicBool,
}

impl HealthRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            checks: Vec::new(),
            started: AtomicBool::new(false),
        }
    }

    /// Add a health check to the registry
    pub fn add_check(&mut self, name: impl Into<String>, check: CheckFn) {
        self.checks.push(HealthCheck {
            name: name.into(),
            check,
        });
    }

    /// Mark the node as started
    pub fn mark_started(&self) {
        self.started.store(true, Ordering::SeqCst);
    }

    /// Check if the node has started
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    /// Run all health checks and return the overall status
    pub async fn check_status(&self) -> StatusResponse {
        let mut checks = HashMap::new();
        let mut all_ok = true;

        for health_check in &self.checks {
            let result = match (health_check.check)().await {
                Ok(()) => CheckResult::ok(),
                Err(e) => {
                    all_ok = false;
                    CheckResult::fail(e)
                }
            };
            checks.insert(health_check.name.clone(), result);
        }

        let status = if !self.is_started() {
            HealthStatus::Starting
        } else if !all_ok {
            if checks.values().all(|c| !c.ok) && !checks.is_empty() {
                HealthStatus::Unhealthy
            } else {
                HealthStatus::Degraded
            }
        } else {
            HealthStatus::Healthy
        };

        StatusResponse { status, checks }
    }

    /// Check if the node is ready (all checks passing and started)
    pub async fn is_ready(&self) -> bool {
        if !self.is_started() {
            return false;
        }

        for health_check in &self.checks {
            if (health_check.check)().await.is_err() {
                return false;
            }
        }

        true
    }
}

impl Default for HealthRegistry {
    fn default() -> Self {
        Self::new()
    }
}
