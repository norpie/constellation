# Constellation Components

## Libraries

- **core** - Shared types and utilities
- **core-derive** - Derive macros
- **fabric** - Transport layer (TCP, protocols)
- **node** - Service framework (mesh participation, Raft consensus, service discovery/transponder, RPC, transport negotiation, resilience patterns)
- **databank** - Database proxy (manages underlying engines, mesh-accessible)
- **shields** - Encryption and security layer

## Runtime Services

- **flux** - Queue system
- **stargate** - Reverse proxy (HTTP to mesh RPC translation)
- **airlock** - DDoS protection layer (frontline, sits before stargate)
- **telemetry** - Metrics and observability system (includes probe components, tracing)
- **bridge** - Orchestration API (cockpit UIs talk to bridge, bridge coordinates everything)
- **cortex** - Deployment orchestrator
- **dispatch** - Container/VM management (interfaces with docker/podman/systemd)
- **quartermaster** - VPS provisioning and procurement
- **rogue** - Chaos engineering (intentional failure injection)
- **autopilot** - LLM-based incident investigation (reads logs, traces, code; suggests fixes)
- **cockpit** - Human interface (webui, cli, tui - thin clients over bridge)
- **cargobay** - CDN and file storage
- **dock** - VM/container image storage and building
- **datapad** - Telemetry data storage
- **fabricator** - CI/CD pipelines
- **vault** - Secrets storage and retrieval
- **lever** - Feature flags

## Far Future

- **beacon** - DNS management
- **nebula** - Virtual networking (VPC, subnets)
- **bulkhead** - Firewall and security groups
- **nexus** - L4 load balancer (TCP/UDP)
- **hold** - Block storage for VMs
- **commons** - Shared file system (NFS-style)
- **archive** - Backup and restore service
- **herald** - Pub/Sub messaging
- **relay** - Event bus
- **forge** - Batch jobs and scheduled tasks
- **stash** - Managed cache (Redis/Memcached)
- **portal** - API gateway
- **manifest** - Service catalog
- **chronicle** - Log aggregation
- **ledger** - Audit trail
- **courier** - Email delivery
