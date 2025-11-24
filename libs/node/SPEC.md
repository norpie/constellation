# Node Framework Specification

This document specifies the design of the `node` library for Constellation's service mesh.

## Overview

The `node` library provides:
- RPC layer over fabric transports
- Raft-based service discovery and address book replication
- Automatic route registration via macros
- Resilient request handling with retries and circuit breakers
- Flexible error handling with opaque payloads

## Route Naming

**Format:** `service.method.version`

**Examples:**
- `UsersService.login.v1`
- `PaymentGateway.process_payment.v2`

**Rules:**
- Not empty
- No underscore prefix (reserved for built-in routes: `_mesh.*`, `_raft.*`, `_telemetry.*`)
- Version defaults to `v1` if not specified

**Generation:**
- Service name: Set via `Node::builder().service_name("UsersService")`
- Method name: Extracted from function name
- Version: Specified in `#[handler(version = N)]`, defaults to 1

**Validation:**
- Underscore check: Compile-time (macro)
- Duplicates: Runtime (node startup, circuit break + telemetry)
- Invalid routes: Deactivated, node continues in degraded mode

## Transponder Data

Data structure advertised when joining mesh:

```rust
struct TransponderData {
    node_id: String,
    addresses: Vec<AddressGroup>,
    transports: Vec<String>,
    codecs: Vec<String>,
    routes: Vec<String>,
    global_constraints: Constraint,
    route_constraints: HashMap<String, Constraint>,
    capabilities: Capabilities,
}

struct AddressGroup {
    zone: String,
    transport: String,
    addresses: Vec<String>,
}

struct Constraint {
    allow_transports: Vec<String>,
    deny_transports: Vec<String>,
    allow_codecs: Vec<String>,
    deny_codecs: Vec<String>,
    allow_combinations: Vec<(String, String)>,  // (transport, codec)
    deny_combinations: Vec<(String, String)>,
}

struct Capabilities {
    can_forward: bool,
    can_translate: bool,
    max_hops: Option<u8>,
}
```

**Node ID:**
- User-provided (config or builder)
- Must be unique across mesh
- On conflict: Node attempts fallback (original_id + random hash)
- Fallback customizable via `id_fallback()` builder method

**Addresses:**
- Multiple transports supported
- Multiple addresses per transport (different zones)
- Zone determines network reachability (e.g., "dc-east", "public", "vpn")
- Order preserved (preference ordering)

**Constraints:**
- Route-specific constraints override global
- Empty lists = no restriction (allow all)
- `allow_combinations` whitelist takes precedence
- Resolution cached per (node, route) on address book update

## Join Process

**Flow:**
1. New node C connects to bootstrap node B (address from config)
2. C sends RPC to `_mesh.join` with transponder data
3. B forwards to Raft leader A
4. A proposes join as Raft log entry
5. Raft commits, state machine adds C to address book
6. Replication propagates to all nodes including C
7. C's join handler observes itself in replicated state → returns success
8. C starts accepting RPC calls

**Raft Membership:**
- **Observer**: Receives replication, no voting, doesn't count toward quorum
- **Voting member**: Full Raft participant, can become leader
- Nodes join as observer initially
- Configurable via `Node::builder().voting_member(bool)` (defaults to true)
- Self-declared (trust-based)

**Built-in Routes:**
- `_mesh.join` - Join mesh
- `_mesh.leave` - Leave mesh (graceful shutdown)
- `_raft.append_entries` - Log replication
- `_raft.request_vote` - Election voting (Yes/No/Idk where Idk = observer)
- `_raft.install_snapshot` - Snapshot transfer
- `_telemetry.*` - Telemetry endpoints

## RPC Call Flow (Direct-Only)

**Caller perspective:**
1. Lookup route in local address book → get list of nodes
2. Apply round-robin selection (per-route state)
3. Select transport+codec via zone > transport > codec priority
4. Check circuit breaker for target node
5. Connect (one-shot for MVP, persistent channels later)
6. Send RpcRequest
7. Wait for RpcResponse with per-attempt timeout
8. On failure: retry with backoff, try next node

**Receiver perspective:**
1. Accept connection (listening on configured transports)
2. Receive RpcRequest
3. Lookup handler for route
4. Extract dependencies (Data<T>, RpcClient)
5. Execute handler (with panic catching)
6. Send RpcResponse

**Message format:**
```rust
struct RpcRequest {
    request_id: Uuid,
    route: String,
    payload: Vec<u8>,  // Codec-encoded
}

struct RpcResponse {
    request_id: Uuid,
    result: ResponseResult,
}

enum ResponseResult {
    Success(Vec<u8>),
    Error {
        category: ErrorCategory,
        payload: Vec<u8>,  // Opaque error bytes
    }
}
```

## Transport/Codec Selection

**Priority order:**
1. **Zone** - Prefer same zone as caller
2. **Transport** - Caller's preference order
3. **Codec** - Caller's preference order

**Algorithm:**
```
shared_zones = caller.zones ∩ target.zones

for zone in shared_zones (caller's order):
    for address_group in target.addresses where zone matches:
        if caller.transports.contains(address_group.transport):
            for codec in caller.codecs:
                if target.codecs.contains(codec):
                    if constraints_allow(route, transport, codec):
                        if !circuit_breaker_open(address_group):
                            return (address_group, codec)

for zone in target.zones not in shared_zones (target's order):
    // Same matching logic

return Error::NoCompatibleTransport
```

**Constraint resolution (cached):**
1. Merge route-specific constraints with global (route overrides)
2. Evaluate allow/deny rules
3. Cache effective constraint per (node, route)
4. Invalidate cache on address book update

## Retry and Resilience

**Configuration:**
```rust
struct ResiliencyConfig {
    max_attempts: u32,
    timeout_per_attempt: Duration,
    total_timeout: Duration,
    backoff: BackoffStrategy,
    circuit_breaker: CircuitBreakerConfig,
}

enum BackoffStrategy {
    Fixed(Duration),
    Linear { start: Duration, increment: Duration },
    Exponential { base: Duration, max: Duration },
    ExponentialJitter { base: Duration, max: Duration },
}

struct CircuitBreakerConfig {
    failure_threshold: u32,
    cooldown: Duration,
}
```

**API:**
```rust
// Use defaults
node.call("route", &request).await?

// Override specific params
node.call("route", &request)
    .max_attempts(5)
    .timeout_per_attempt(Duration::from_secs(2))
    .backoff(BackoffStrategy::Exponential { ... })
    .await?
```

**Round-robin state:**
- Per-route tracking: `HashMap<Route, AtomicUsize>`
- Reset on address book update
- Thread-safe for concurrent callers

**Retryable errors:**
- Connection refused/failed
- Timeout
- ErrorCategory::Retryable
- ErrorCategory::ServerError (try different node)
- ErrorCategory::Unavailable (try different node immediately)

**Non-retryable:**
- ErrorCategory::ClientError
- Invalid route (not in address book)
- No compatible transport+codec

## Handler API

**Definition:**
```rust
#[handler]
async fn login(
    req: LoginRequest,
    db: Data<DbPool>,
    rpc: RpcClient,
) -> Result<LoginResponse, MyError> {
    // Handler logic
}

#[handler(version = 2)]
async fn create_account(req: CreateRequest) -> Result<()> {
    // ...
}
```

**Extractors:**
- `Data<T>`: Shared application state (registered via `Node::builder().data(value)`)
- `RpcClient`: Make outbound RPC calls to other services
- Request type (first parameter): Automatically decoded via codec

**Registration:**

*Production (automatic):*
```rust
Node::builder()
    .service_name("UsersService")
    .data(db_pool)
    .build();  // Auto-discovers all #[handler] functions via inventory
```

*Tests (manual):*
```rust
Node::builder()
    .service_name("UsersService")
    .auto_discover(false)
    .register("login.v1", &LOGIN_HANDLER)
    .data(db_pool)
    .build();
```

**Macro expansion:**
```rust
#[handler]
async fn login(...) -> Result<Response, Error> { ... }

// Expands to:

struct LoginHandler;

impl Handler for LoginHandler {
    async fn call(&self, node: &Node, request: &RpcRequest)
        -> Result<Vec<u8>>
    {
        // Decode request with bincode
        let codec = BincodeCodec;
        let req: LoginRequest = codec.decode(&request.payload)?;

        // Extract dependencies from node.data
        let db: Data<DbPool> = node.extract().ok_or(...)?;

        // Call actual handler function
        let response = login(req, db).await
            .map_err(|e| Error::Custom(e.to_string()))?;

        // Encode response with bincode
        codec.encode(&response)
    }
}

inventory::submit! {
    HandlerRegistration::new("login", 1, LoginHandler)
}

pub const LOGIN_HANDLER: LoginHandler = LoginHandler;
```

**Built-in handlers:**
- Implemented as regular Handler trait impls
- Registered in `Node::build()` before user handlers
- Use same Data<T> extractor mechanism
- Internal state: `Data<RaftState>`, `Data<AddressBook>`, etc.

## Error Handling

**Traits:**
```rust
trait Responder: Serialize {}

trait ErrorResponder: Serialize {
    fn error_category(&self) -> ErrorCategory;
}

enum ErrorCategory {
    Retryable,        // Temporary issue, retry same/different node, no circuit breaker
    ServerError,      // This node has issues, try different node, increment circuit breaker
    ClientError,      // Bad request, don't retry, no circuit breaker
    Timeout,          // Deadline exceeded, retry different node, increment circuit breaker
    Unavailable,      // Service degraded, try different node immediately, increment circuit breaker
}
```

**Auto-implementations:**
```rust
impl<T: Serialize> Responder for T {}

impl ErrorResponder for anyhow::Error {
    fn error_category(&self) -> ErrorCategory {
        ErrorCategory::ServerError  // Conservative default
    }
}
```

**Custom errors:**
```rust
#[derive(Serialize)]
enum MyError {
    RateLimit,
    NotFound,
    DatabaseDown,
}

impl ErrorResponder for MyError {
    fn error_category(&self) -> ErrorCategory {
        match self {
            Self::RateLimit => ErrorCategory::Retryable,
            Self::NotFound => ErrorCategory::ClientError,
            Self::DatabaseDown => ErrorCategory::ServerError,
        }
    }
}
```

**Panic handling:**
- Caught via `AssertUnwindSafe` wrapper
- Returns `ErrorCategory::ServerError`
- Generic "Internal Server Error" response
- Logged to telemetry with panic info
- Circuit breaker incremented

**Wire format:**
- Success: opaque bytes (caller decodes to expected type)
- Error: category + opaque bytes (caller decodes to error type)
- Framework uses category for retry logic only
- Never inspects error payload contents

## Data Extraction Mechanism

**Storage:**
```rust
struct Node {
    data: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    // ...
}
```

**Registration:**
```rust
impl Node {
    pub fn data<T: 'static + Send + Sync>(mut self, value: T) -> Self {
        let data = Data(Arc::new(value));
        self.data.insert(TypeId::of::<Data<T>>(), Box::new(data));
        self
    }

    fn extract<T: 'static>(&self) -> Option<Data<T>> {
        self.data
            .get(&TypeId::of::<Data<T>>())
            .and_then(|any| any.downcast_ref::<Data<T>>())
            .cloned()  // Cheap Arc clone
    }
}
```

**Usage in generated code:**
```rust
// In macro-generated Handler::call:
let db: Data<DbPool> = node.extract()
    .ok_or(RpcError::MissingDependency)?;
```

## Codec Strategy

**Current Implementation (MVP):**
All handlers use `BincodeCodec` directly. This avoids object-safety issues with generic traits and keeps the implementation simple.

```rust
// In generated handler code:
let codec = BincodeCodec;
let req: LoginRequest = codec.decode(&request.payload)?;
// ... handler logic ...
codec.encode(&response)?
```

**Future: Codec Factory Pattern (Deferred)**

The `Codec` trait has generic methods, making it not object-safe (`Box<dyn Codec>` is not allowed). A factory pattern could solve this, but introduces complexity:

```rust
// Investigated but not implemented:
pub trait CodecFactory: Send + Sync {
    fn create_typed<Req, Resp>(&self) -> Box<dyn TypedCodec<Req, Resp>>;
}
// Problem: CodecFactory itself is not object-safe (generic method)
```

**Why Deferred:**
- Object-safety issues cascade: solving Codec requires TypedCodec factory, which itself isn't object-safe
- Bincode is sufficient for MVP
- Can revisit when codec pluggability becomes a requirement
- May require rethinking the entire approach (e.g., macro-based codec selection)

## Address Book

**Structure:**
- Stored in Raft state machine
- Replicated to all nodes (observers + voting members)
- Maps: node_id → TransponderData
- Maps: route → Vec<node_id> (reverse index)

**Updates:**
- Join: Add node via Raft proposal
- Leave: Remove node via Raft proposal
- Heartbeat failures: Leader proposes removal (TBD)

**Caching:**
- Effective constraints: (node_id, route) → Constraint
- Rebuilt on address book update
- Used by transport/codec selection

## Implementation Notes

**Crate organization:**
```
constellation-node-derive/  - Proc macro crate for #[handler]
constellation-raft/         - Separate crate, generic Raft implementation
constellation-node/         - Public API, RPC, routing, mesh integration
  src/
    codec.rs            - TypedCodec, CodecFactory, built-in factories
    handler.rs          - Handler trait, registration
    rpc.rs              - RPC request/response types, ErrorCategory
    error.rs            - Error types
    lib.rs              - Re-exports, Node struct placeholder
```

**Testing:**
- Inventory auto-discovery disabled in test builds
- Manual registration via `.register()` and `.auto_discover(false)`
- Can spawn multiple nodes in single test process

**Constraints:**
- 1 service = 1 binary in production
- Tests can run multiple services via manual registration
- Service name set at node build time
- All handlers prepended with service name

## Future Extensions

**Not in MVP, revisit later:**
- Circuit breaker implementation (hooks present via ErrorCategory, actual breaker logic deferred)
- Persistent connections / multiplexing
- Multi-hop routing / forwarding
- Protocol translation nodes
- Load balancing beyond round-robin
- Automatic codec benchmarking
- Canary / blue-green deployment support
- Request shadowing
