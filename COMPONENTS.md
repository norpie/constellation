# Constellation Components

## Libraries

- **core** - Shared types and utilities
- **core-derive** - Derive macros
- **fabric** - Transport layer (TCP, protocols)
- **node** - Service framework (mesh participation, Raft consensus, service discovery/transponder, RPC, transport negotiation, resilience patterns)
- **databank** - Database abstraction and driver layer
- **shields** - Encryption and security layer

## Runtime Services

- **flux** - Queue system
- **stargate** - Reverse proxy
- **airlock** - DDoS protection layer
- **telemetry** - Metrics and observability system (includes probe components)
- **bridge** - High-level orchestration between cockpit, quartermaster, cortex, and dispatch
- **cortex** - Deployment orchestrator
- **dispatch** - Container/VM management (interfaces with docker/podman/systemd)
- **quartermaster** - VPS provisioning and procurement
- **autopilot** - LLM-based incident response suggestions
- **cockpit** - Dashboard and human interface
- **cargobay** - CDN and file storage
- **dock** - VM/container image storage and building
- **datapad** - Telemetry data storage
