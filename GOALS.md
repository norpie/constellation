# Goals

**Transport layer specifics:**
- Trait-based pluggable system allowing downstream custom implementations
- Per-service transport configuration (config-based or code-based)
- Protocol negotiation and advertisement mechanics
- Translation/routing nodes for protocol bridging
- RPC-style communication pattern
- Examples: bincode, protobuf, queue-based, custom satellite link

**Service discovery details:**
- All services participate in Raft (not dedicated discovery nodes)
- Address book structure: multiple address types (IPv4, IPv6, socket paths, VPN constraints)
- Endpoint versioning scheme ("services.users.login.v2")
- Leader doesn't need direct connectivity to all nodes
- Self-election vs provided leader bootstrapping

**Deployment specifics:**
- Three deployment targets: NixOS, Debian/apt (declarative-on-imperative), containers
- Declarative spec format with constraints and autoscaling rules
- Stack-level defaults with per-service overrides
- Example spec: "3 user services on NixOS, scale to 10 at 50% load"

**Storage layer:**
- dock: image building and storage
- cargobay: CDN/centralized file management
- databank: database drivers, possibly Diesel adapters
- datapad: telemetry storage backend
- Separate caching layer concept (redis/memcache + custom for eviction experiments)

**Observability:**
- Hybrid push/pull (pull normally, push on critical events)
- "probe" components within telemetry
- Unified interface goal (not scattered tooling)
- OpenTelemetry compatibility alongside custom experiments

**Security:**
- Inter-service encryption: TLS over TCP or plain over VPN
- Mesh join authentication (method undefined)
- Circuit breakers, retries, timeouts in service library
- shields library scope

**Auto-scaling mechanics:**
- Metrics → decision → quartermaster buys VPS → deployment provisions
- Rules-based vs reconciliation loops
- Manual override capabilities

**Using existing tools:**
- tokio (async), hyper (HTTP base), docker/podman (containers)
