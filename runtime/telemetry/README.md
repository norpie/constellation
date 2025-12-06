# Telemetry

Observability for Constellation. Collects metrics, logs, and traces with unified correlation.

## Crate Structure

```
libs/
├── telemetry/         # Collection API, macros, context propagation
└── datapad/           # Unified storage engine (built on sled)

runtime/
└── telemetry/         # Service that scrapes, stores, serves
```

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

---

# Ingestion

Hybrid pull/push model. Normal telemetry is scraped; exceptional events push immediately.

## Node-Side: Collector

```
┌─────────────────────────────────────────────────────────────────┐
│  Node                                                           │
│                                                                 │
│  info!("msg") ───►  Collector                                   │
│                        │                                        │
│                        ├─── level in immediate_push_levels?     │
│                        │         │                              │
│                        │        yes ──► RPC _telemetry.ingest   │
│                        │                                        │
│                        no                                       │
│                        │                                        │
│                        ▼                                        │
│                   MemBuffer                                     │
│                        │                                        │
│                        │ full?                                  │
│                        ▼                                        │
│                      WAL (on-disk overflow)                     │
│                                                                 │
│  _telemetry.scrape ◄──── Telemetry service (periodic)           │
│         │                                                       │
│         └──► drain MemBuffer + WAL, return entries              │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Collector Config

```rust
CollectorConfig {
    // Memory buffer
    memory_buffer_size: usize,          // Max entries before overflow to WAL

    // WAL overflow (disk-based durability)
    wal_enabled: bool,
    wal_directory: PathBuf,
    wal_max_file_size: u64,             // Rotate after this size
    wal_max_total_size: u64,            // Delete oldest when exceeded

    // Immediate push triggers
    immediate_push_levels: Vec<Level>,  // e.g., [Error] or [Error, Warn]
}
```

### WAL (Write-Ahead Log)

When memory buffer overflows or push fails, entries spill to disk:

- **Append-only**: Sequential writes only (fast)
- **Batched + buffered**: Accumulate before write, periodic fsync
- **Length-prefixed frames**: `[len: u32][checksum: u32][payload]`
- **Rotation**: New file when max size reached
- **Cleanup**: Delete oldest files when total size exceeded

Scrape reads from both memory buffer and WAL tail.

## Telemetry Service Side

```
┌─────────────────────────────────────────────────────────────────┐
│  Telemetry Service                                              │
│                                                                 │
│  Scraper (per registered node):                                 │
│    loop {                                                       │
│        sleep(scrape_interval)                                   │
│        entries = RPC _telemetry.scrape(node_addr)               │
│        notify_subscribers(entries)                              │
│        datapad.insert_batch(entries)                            │
│    }                                                            │
│                                                                 │
│  _telemetry.ingest handler (immediate push):                    │
│    validate(entries)                                            │
│    notify_subscribers(entries)                                  │
│    datapad.insert_batch(entries)                                │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Built-in Handlers (Node)

| Route | Purpose |
|-------|---------|
| `_telemetry.scrape` | Returns buffered entries, clears buffer |
| `_telemetry.ingest` | Receives immediate push (on Telemetry service) |

## Failure Handling

**Telemetry service down:**
1. Immediate push fails → entries go to WAL instead
2. Memory buffer fills → overflow to WAL
3. WAL accumulates until scrape resumes
4. WAL exceeds max size → delete oldest (lose old data, keep recent)

**Node restarts:**
- WAL survives restart
- On startup, Collector loads WAL, resumes from last position
- Next scrape picks up persisted entries

---

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

## Context Propagation

Telemetry context flows automatically through async code via task-local storage.

```rust
// Handler macro creates context and scopes it
async fn __handler_wrapper(req: Request) -> Response {
    let ctx = TelemetryContext::new(trace_id, span_id, ...);
    TELEMETRY_CONTEXT.scope(ctx, async {
        actual_handler(req).await
    }).await
}

// Any function called from handler can access context
fn helper() {
    info!("works");  // Reads from task-local, has trace_id
}
```

For spawned tasks, use `constellation::spawn()` instead of `tokio::spawn()`:

```rust
pub fn spawn<F>(future: F) -> JoinHandle<F::Output> {
    let ctx = TelemetryContext::current();  // Capture
    tokio::spawn(async move {
        TELEMETRY_CONTEXT.scope(ctx, future).await  // Restore
    })
}
```

## Automatic Context

The framework automatically attaches to all telemetry:

- `timestamp`
- `service`, `node_id`
- `trace_id`, `span_id` (when in span context)

---

# Data Model

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Timestamp | Microseconds (u64) | Sub-ms precision, reasonable size |
| Entry ID | ULID | Time-sorted, compact (26 chars), no coordination |
| Trace ID | 128-bit hex (32 chars) | OpenTelemetry compatible |
| Span ID | 64-bit hex (16 chars) | OpenTelemetry compatible |
| Tags | Hybrid (columns + HashMap) | Fast queries on common fields, flexible extras |
| Indexing | Configurable | User chooses which tags to index |
| Histograms | Raw + rollups | Full fidelity recent, aggregated historical |

## Common Fields (all entry types)

| Field | Type | Indexed | Notes |
|-------|------|---------|-------|
| `id` | ULID | Primary | Time-sorted unique identifier |
| `timestamp` | u64 | Yes | Microseconds since Unix epoch |
| `service` | String | Yes | Service name |
| `node_id` | String | Yes | Instance identifier |
| `trace_id` | Option<String> | Yes | 128-bit hex, OpenTelemetry format |
| `span_id` | Option<String> | No | 64-bit hex, OpenTelemetry format |
| `tags` | HashMap<String, String> | Configurable | Arbitrary key-value pairs |

## Log Entry

| Field | Type | Indexed | Notes |
|-------|------|---------|-------|
| `level` | Level | Yes | Error, Warn, Info, Debug |
| `message` | String | No | The log message |
| `target` | Option<String> | No | Module path / logger name |
| `file` | Option<String> | No | Source file |
| `line` | Option<u32> | No | Source line |

## Metric Entry

| Field | Type | Indexed | Notes |
|-------|------|---------|-------|
| `name` | String | Yes | e.g., `requests_total` |
| `metric_type` | MetricType | No | Counter, Gauge, Histogram |
| `value` | f64 | No | Current value (counter/gauge) |
| `histogram` | Option<Vec<f64>> | No | Raw samples for histograms |

## Span Entry

| Field | Type | Indexed | Notes |
|-------|------|---------|-------|
| `name` | String | Yes | Operation name |
| `parent_span_id` | Option<String> | Yes | For building trace tree |
| `start` | u64 | No | Start timestamp (micros) |
| `end` | u64 | No | End timestamp (micros) |
| `status` | SpanStatus | No | Ok, Error |

---

# Storage (Datapad)

Unified telemetry database built on sled. Single store for all three types.

## Primary Storage

```
Key:   [timestamp_micros: u64][entry_type: u8][ulid: 128-bit]
Value: TelemetryEntry (bincode encoded)

pub enum TelemetryEntry {
    Log(LogEntry),
    Metric(MetricEntry),
    Span(SpanEntry),
}
```

Time-sorted keys mean range scans are fast.

## Secondary Indices

Built-in indices (always created):

| Index | Key → Value |
|-------|-------------|
| `trace_index` | trace_id → Vec<primary_key> |
| `service_index` | service → Vec<primary_key> |
| `metric_name_index` | metric_name → Vec<primary_key> |
| `level_index` | log_level → Vec<primary_key> |

User-configured indices for high-cardinality tags:

```rust
DatapadConfig {
    indexed_tags: vec!["user_id", "endpoint", "customer_id"],
}
```

## Histogram Aggregation

Raw samples stored for recent data, rollups computed on schedule:

```
┌─────────────────────────────────────────────────────────┐
│  Raw samples                                            │
│  - Full fidelity, last 24 hours                         │
├─────────────────────────────────────────────────────────┤
│  1-minute rollups                                       │
│  - count, sum, min, max, p50, p90, p99                  │
│  - Last 7 days                                          │
├─────────────────────────────────────────────────────────┤
│  1-hour rollups                                         │
│  - Same aggregates                                      │
│  - Last 90 days                                         │
├─────────────────────────────────────────────────────────┤
│  1-day rollups                                          │
│  - Same aggregates                                      │
│  - Forever                                              │
└─────────────────────────────────────────────────────────┘
```

Scheduled tasks:
- `aggregate`: raw → 1min → 1hr → 1day
- `cleanup`: delete data older than retention policy

## Retention Policies

Configurable per entry type:

```rust
RetentionConfig {
    logs: Duration::days(30),
    metrics_raw: Duration::hours(24),
    metrics_1min: Duration::days(7),
    metrics_1hr: Duration::days(90),
    metrics_1day: None,  // Forever
    spans: Duration::days(14),
}
```

## Query API

```rust
datapad.query()
    .time_range(start, end)
    .entry_type(EntryType::Log)
    .service("auth")
    .filter_tag("user_id", "123")
    .execute()
    .await?;

// Correlation query - everything for a trace
datapad.query()
    .trace_id("abc123def456...")
    .execute()
    .await?;  // Returns logs, metrics, spans
```

---

# Query Language

Terse, pipeline-based DSL inspired by PromQL/LogQL. Lives at the Telemetry service level - parses query strings and translates to Datapad builder calls.

## Syntax

```
selector | filter | filter | aggregation | timerange
```

### Selectors

```
logs{service="auth"}              # Logs from auth service
metrics{name="request_duration"}  # Specific metric
spans{service="api"}              # Spans from api service
*{trace_id="abc123"}              # All types for a trace
```

### Label Matching

```
{service="auth"}          # Exact match
{service!="internal"}     # Not equal
{service=~"auth|api"}     # Regex match
{service!~"test.*"}       # Regex not match
```

### Filters (pipeline stages)

```
# Log content filters
logs{service="auth"} |= "error"           # Contains string
logs{service="auth"} |~ "user_\d+"        # Regex match

# Field filters (any type)
logs{service="auth"} | level=error
spans{service="api"} | duration > 100ms
metrics{name="latency"} | value > 1000
```

### Aggregations

```
# Metrics aggregations
metrics{name="requests"} | rate(5m)
metrics{name="latency"} | avg
metrics{name="latency"} | p99
metrics{name="errors"} | sum | by(service)

# Counting
logs{service="auth"} | level=error | count
logs{service="auth"} | count | by(level)
```

### Time Ranges

```
logs{service="auth"} | last 1h
logs{service="auth"} | last 30m
spans{service="api"} | from 2024-01-01T00:00:00Z to 2024-01-02T00:00:00Z
```

## Example Queries

```
# Error rate by service (last hour)
metrics{name="errors_total"} | rate(5m) | by(service) | last 1h

# Slow endpoints
spans{service="api"} | duration > 500ms | by(endpoint) | last 30m

# Recent auth failures
logs{service="auth"} |= "failed" | level=error | last 15m

# P99 latency by endpoint
metrics{name="request_duration"} | p99 | by(endpoint) | last 1h

# Everything for debugging a request
*{trace_id="abc123def456"}

# Errors containing specific text
logs{service="payments"} |= "timeout" | level=error | last 6h

# High-cardinality drill-down
logs{service="auth", user_id="12345"} | last 24h
```

## Grammar

```
query       = selector pipeline?
selector    = type "{" labels "}"
type        = "logs" | "metrics" | "spans" | "*"
labels      = label ("," label)*
label       = key op value
op          = "=" | "!=" | "=~" | "!~"

pipeline    = ("|" stage)*
stage       = filter | aggregation | timerange

filter      = "|=" string                    # contains (logs)
            | "|~" string                    # regex (logs)
            | key compare_op value           # field comparison

compare_op  = "=" | "!=" | ">" | ">=" | "<" | "<="

aggregation = "rate" "(" duration ")"
            | "avg" | "sum" | "min" | "max" | "count"
            | "p50" | "p90" | "p95" | "p99"
            | "by" "(" key ("," key)* ")"

timerange   = "last" duration
            | "from" timestamp "to" timestamp

duration    = number ("s" | "m" | "h" | "d")
```

---

# Client Telemetry

Browser-side JS library for collecting end-user performance metrics.

## Architecture

```
┌─────────────────────────────────────────────┐
│  Browser (JS lib)                           │
│  - Collects metrics, batches, anonymizes    │
└─────────────────┬───────────────────────────┘
                  │ HTTPS POST /ingest
┌─────────────────▼───────────────────────────┐
│  Stargate                                   │
│  - Rate limiting, validation                │
└─────────────────┬───────────────────────────┘
                  │ RPC
┌─────────────────▼───────────────────────────┐
│  Telemetry service                          │
│  - Additional anonymization, store          │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│  Datapad (source="client" tag)              │
└─────────────────────────────────────────────┘
```

Same storage as server telemetry, tagged with `source="client"` for:
- Different retention policies (shorter)
- Separate rate limits
- Query filtering

## What to Collect

**Performance (Core Web Vitals):**
- LCP (Largest Contentful Paint)
- FID (First Input Delay) / INP (Interaction to Next Paint)
- CLS (Cumulative Layout Shift)
- TTFB (Time to First Byte)
- Page load timing

**Errors:**
- JS exceptions (sanitized stack traces)
- Failed network requests
- Console errors

**Custom events:**
- User interactions (navigation, clicks)
- Business events (conversion, checkout)

## Anonymization

| Data | Risk | Mitigation |
|------|------|------------|
| IP address | PII, geolocation | Hash or strip, keep only country/region |
| User agent | Fingerprinting | Normalize to browser + OS only |
| URLs | Query params may contain PII | Strip query params, allowlist safe ones |
| User ID | Direct PII | Hash with daily rotating salt |
| Session ID | Tracking | Short-lived, rotating, no cross-session linking |
| Stack traces | May contain user data | Sanitize URLs, redact variables |
| Custom events | User-controlled | Schema validation, reject unexpected fields |

### Strategies

**Hash identifiers:**
```js
// Client-side before sending
session_id: sha256(real_session + daily_salt)
```

**URL sanitization:**
```js
// Before: /users/123/profile?email=foo@bar.com
// After:  /users/[id]/profile
```

**Client-side aggregation:**
```js
// Batch events, summarize before sending
{ event: "clicks", count: 12, page: "/home", period: "1m" }
```

**Sampling:**
```js
// Only collect 10% of sessions
if (Math.random() > 0.1) return;
```

**Consent-aware:**
```js
Telemetry.init({
  enabled: userConsent.analytics,
  anonymize: true,
});
```

## Trust Boundary

Client-sent data cannot be trusted. Server-side protections:

- **Rate limiting** - Per IP, per session
- **Schema validation** - Reject malformed/unexpected payloads
- **Size limits** - Max payload size, max events per batch
- **Separate quotas** - Client telemetry doesn't affect server telemetry limits

## Retention

Client telemetry has shorter retention by default:

```rust
RetentionConfig {
    // Server telemetry
    logs: Duration::days(30),
    spans: Duration::days(14),

    // Client telemetry (source="client")
    client_metrics: Duration::days(7),
    client_errors: Duration::days(14),
    client_events: Duration::days(3),
}
```

## Correlation

Client telemetry can link to server traces via `trace_id`:

```js
// Browser receives trace_id from response header
fetch('/api/checkout')
  .then(res => {
    const traceId = res.headers.get('X-Trace-Id');
    Telemetry.setTraceId(traceId);
    // Subsequent client events tagged with this trace_id
  });
```

Query everything for a user journey:
```
*{trace_id="abc123"}  # Returns server spans + client metrics
```

---

# Subscriptions

Services subscribe to telemetry events for real-time reactions:

- **Autopilot**: Investigates errors, suggests fixes
- **Rogue**: Monitors steady state during chaos testing
- **Cockpit**: Live dashboards, alerts
- **Alerts**: Threshold-based notifications

---

# Sampling

Tail-based sampling with guaranteed capture of interesting traces.

## Strategy

1. Buffer spans briefly in Telemetry service
2. When trace completes (or times out), evaluate:
   - Has errors? → **Keep 100%**
   - Exceeds latency threshold? → **Keep 100%**
   - Otherwise → **Sample at configured rate**

## Config

```rust
SamplingConfig {
    // Base sample rate for "uninteresting" traces
    base_rate: f32,                    // e.g., 0.1 = 10%

    // Always keep these
    always_keep_errors: bool,          // true
    always_keep_slow: bool,            // true
    slow_threshold: Duration,          // e.g., 500ms

    // Buffer settings
    trace_buffer_timeout: Duration,    // How long to wait for trace completion
}
```

Sampling decisions happen in Telemetry service before writing to Datapad.

---

# Alerts

Alert rules defined in Telemetry service config. Uses the query language with thresholds.

## Rule Definition

```yaml
alerts:
  - name: high_error_rate
    query: 'metrics{name="errors_total"} | rate(5m) | by(service)'
    condition: "> 0.05"
    for: 5m                # Sustained duration before firing
    severity: critical

  - name: slow_p99
    query: 'metrics{name="request_duration"} | p99 | by(service)'
    condition: "> 1000"    # > 1 second
    for: 10m
    severity: warning

  - name: disk_pressure
    query: 'metrics{name="wal_size_bytes", source="collector"}'
    condition: "> 500000000"  # 500MB
    for: 1m
    severity: warning
```

## Evaluation

Telemetry service runs alert queries on schedule (e.g., every 30s):
1. Execute query
2. Compare result to condition
3. Track "firing" state and duration
4. When `for` duration exceeded → alert fires

## Notification

**TBD**: Notification delivery depends on pubsub system (not yet designed).

For now, fired alerts:
- Logged as telemetry events (can query: `logs{service="telemetry"} |= "alert:high_error_rate"`)
- Exposed via `_telemetry.alerts` handler (list active alerts)

Future: pubsub subscribers receive alert events in real-time.

---

# Health Checks

Health probes are **separate from telemetry** - they're node framework concerns used by orchestrators and load balancers.

## Handlers (in Node Framework)

| Route | Purpose | Returns |
|-------|---------|---------|
| `_health.live` | Is process alive? | `{ ok: bool }` |
| `_health.ready` | Can serve traffic? | `{ ok: bool, reason?: string }` |
| `_health.startup` | Finished initializing? | `{ ok: bool }` |

Services implement readiness logic:
```rust
Node::builder()
    .readiness_check(|| async {
        db.ping().await.is_ok() && cache.ping().await.is_ok()
    })
```

## Health as Metrics

Health check results are **emitted as metrics** to telemetry:

```rust
// On each health check (internal or scraped)
metric!(gauge "health_live", if live { 1.0 } else { 0.0 });
metric!(gauge "health_ready", if ready { 1.0 } else { 0.0 });
```

Query health history:
```
metrics{name="health_ready", service="auth"} | last 1h
```

Alert on health:
```yaml
alerts:
  - name: service_not_ready
    query: 'metrics{name="health_ready"} | by(service, node_id)'
    condition: "== 0"
    for: 1m
    severity: critical
```

---

# Export (Deferred)

Export to external systems (Prometheus, Jaeger, OTLP) is **deferred**.

Design accommodates future export:
- Trace/span IDs use OpenTelemetry format (128-bit / 64-bit hex)
- Data model maps cleanly to OTLP
- Could add `/metrics` Prometheus endpoint or OTLP exporter later

---

# Open Questions

- **Pubsub**: How do services subscribe to events (alerts, telemetry streams)?
