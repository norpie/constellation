# Rogue

Chaos engineering service for Constellation. Intentionally causes failures to verify system resilience.

## Concept

Rogue executes chaos rules against the mesh, targeting different levels of granularity:

| Level | Target | Example |
|-------|--------|---------|
| Handler | Single RPC endpoint | Disable `auth.login.v1` |
| Node | Service instance | Kill `auth-1` |
| Service | All instances of type | Kill all `AuthService` nodes |
| Cluster | Raft group | Partition the leader |

## Actions

- **Kill** - graceful (`_manage.shutdown`) or ungraceful (`_manage.kill`)
- **Disable** - disable specific handlers temporarily
- **Latency** - inject artificial delay
- **Drop** - drop percentage of requests
- **Partition** - block communication between targets

## Rules

Rules define what chaos to cause, when, and within what limits:

- **Target**: what to hit (level, selector, pick strategy)
- **Action**: what to do
- **Schedule**: when (cron, interval, manual)
- **Constraints**: blast radius limits (max concurrent, max percent, maintain quorum, exclusions)
- **Abort**: telemetry alerts to subscribe to, timeout

## Integration

- Discovers targets via address book
- Calls `_manage.*` handlers on nodes (or dispatch for container-level)
- Emits events to telemetry (`rogue.event.started`, `rogue.event.completed`, `rogue.event.aborted`)
- Subscribes to telemetry alerts for abort signals

## Philosophy

Rogue stays dumb. It executes chaos and emits events. Analysis, reporting, and dashboards are queries against telemetry data, not Rogue's job.
