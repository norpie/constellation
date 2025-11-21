# Constellation

Experimental microservices platform built from scratch in Rust.

Research/learning project exploring custom service communication protocols, infrastructure components, platform automation, and observability tooling.

## How It Works

**Service Mesh:** Services join a mesh via Raft consensus. The elected leader (transponder) maintains an address book of all services, their locations, and supported transport protocols. Services can communicate using different transports (bincode, protobuf, queues) with automatic protocol negotiation.

**External Requests:** HTTP requests hit the reverse proxy (stargate), protected by DDoS mitigation (airlock). Gateway services translate HTTP to internal service calls and route through the mesh.

**Discovery:** On startup, services connect to a known leader or self-elect. The leader propagates mesh topology. Services resolve dependencies through the address book, which includes routing hints (IPs, VPN requirements, socket paths, transport capabilities).

**Deployment:** Declare desired state (services, counts, constraints). The orchestration layer (bridge + cortex) coordinates with quartermaster for VPS procurement and dispatch for container/VM operations. Telemetry feeds autoscaling decisions.

**Transport Flexibility:** Services advertise supported protocols. The mesh can route through translation nodes when direct communication isn't possible. Some services may only support specific transports due to network constraints.

See [COMPONENTS.md](COMPONENTS.md) for the full component list and [GOALS.md](GOALS.md) for implementation goals and architectural details.
