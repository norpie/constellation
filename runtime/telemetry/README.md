# Telemetry

Observability for Constellation. Collects metrics, logs, and traces with unified correlation.

## Three Pillars

| Type | What | Example |
|------|------|---------|
| Metrics | Numbers over time | `request_count`, `latency_p99` |
| Logs | Discrete events | `"User logged in"`, `"Connection refused"` |
| Traces | Request flow | Spans forming a tree across services |

All three share arbitrary key-value tags for correlation.

## Collection Model

- **Pull**: Telemetry periodically scrapes services (normal operation)
- **Push**: Services push exceptional events (errors, threshold breaches)
- **Subscribe**: Consumers subscribe to real-time notifications

## Developer API

The `#[handler]` macro injects telemetry context automatically. No explicit parameters needed.

### Logging

```rust
info!("Processing request", "user_id" => user_id);
warn!("Rate limit approaching", "current" => count, "max" => 100);
error!("Connection failed", "host" => host, "error" => err);
debug!("Cache miss", "key" => key);
```

Levels: `error!`, `warn!`, `info!`, `debug!`

### Metrics

```rust
metric!(counter "requests_total");
metric!(counter "errors_total", "type" => "timeout");
metric!(gauge "active_connections", 42.0);
metric!(histogram "request_duration_ms", elapsed);
```

Types: counter (inc), gauge (set), histogram (record)

### Spans

```rust
// Tag current span
span!("user_id" => user_id, "request_type" => "login");

// Child span with block
span!(child "database_query", "table" => "users", {
    let result = db.query(...).await?;
});
```

### Full Example

```rust
#[handler]
async fn login(req: LoginRequest) -> Result<LoginResponse, Error> {
    span!("user" => &req.username);
    info!("Login attempt", "user" => &req.username);
    metric!(counter "login_attempts_total");

    let user = span!(child "fetch_user", {
        db.find_user(&req.username).await?
    });

    if !user.verify_password(&req.password) {
        warn!("Invalid password", "user" => &req.username);
        metric!(counter "login_failures_total", "reason" => "bad_password");
        return Err(Error::InvalidCredentials);
    }

    info!("Login successful", "user" => &req.username);
    metric!(counter "login_success_total");
    Ok(LoginResponse { token: user.generate_token() })
}
```

## Automatic Context

The framework automatically attaches to all telemetry:

- `timestamp`
- `service`, `node_id`
- `trace_id`, `span_id` (when in span context)

## Storage (Datapad)

Single database optimized for metrics/logs/traces with:

- Unified time-series indexing
- Secondary indices on tags (trace_id, service, user_id, etc.)
- Native correlation queries across all three types

```sql
-- Everything for one request
SELECT * FROM telemetry WHERE trace_id = 'abc123'

-- Errors with context
SELECT * FROM logs
WHERE level = 'error' AND tags->>'service' = 'auth'
```

## Subscriptions

Services subscribe to telemetry events for real-time reactions:

- **Autopilot**: Investigates errors, suggests fixes
- **Rogue**: Monitors steady state during chaos
- **Cockpit**: Live dashboards, alerts
- **Alerts**: Threshold-based notifications

## Open Questions

- **Sampling**: At high volume, store everything or sample traces?
- **Aggregation**: Raw metrics or pre-aggregated rollups (1min, 5min, 1hr)?
- **Retention**: How long is data kept? Different policies for metrics/logs/traces?
- **Buffering**: If telemetry is down, do services buffer locally?
- **Cardinality**: High-cardinality tags (user_id) can explode storage. Limits?
- **Alerts**: Where are alert rules defined? Part of telemetry config?
- **Health checks**: Liveness/readiness probes - part of telemetry or separate?
- **Export**: Push to external systems (Prometheus, Grafana, etc.)?
