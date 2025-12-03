# Security

Internal authentication and authorization for the Constellation mesh.

## Overview

Every node in the mesh has a cryptographic identity. All communication is authenticated and optionally encrypted. Permissions are granted by a human admin via signed certificates.

## Identity

Nodes prove identity using public key cryptography.

**Keypair generation:**
- Each node generates an Ed25519 keypair on creation
- Node ID can be derived from public key: `node_id = base58(sha256(public_key))`
- Private key stored locally (file, encrypted file, or hardware TPM/HSM)

**Node files:**
```
/etc/constellation/
├── node.key          # node's private key
└── node.grant        # signed permission grant from admin
```

## Authentication

Every interaction requires cryptographic proof.

**Message signing:**
```
Sender:
  1. Create message
  2. Add nonce + timestamp
  3. Sign with private key
  4. Send

Receiver:
  1. Verify signature against sender's public key (from address book)
  2. Check nonce hasn't been used (prevents replay)
  3. Check timestamp is recent
  4. If valid → process
```

**Challenge-response on connection:**
```
A connects to B:
  1. B sends random challenge
  2. A signs challenge with private key
  3. B verifies signature
  4. (Optional: mutual - A challenges B too)
```

## Encryption

Optional but recommended. Uses hybrid encryption for performance.

**Connection establishment:**
1. Authenticate via challenge-response (signatures)
2. Key exchange (X25519) to derive shared secret
3. Derive symmetric session key (HKDF)

**Message encryption:**
1. Encrypt with session key (ChaCha20-Poly1305 or AES-GCM)
2. Sign the encrypted payload
3. Send

Framework handles this transparently - developers just call RPC methods.

**Configuration:**
```rust
Node::builder()
    .identity(Identity::from_file("node.key")?)
    .security(Security::SignAndEncrypt)  // or SignOnly, or None for dev
```

## Imposter Detection

If a private key is compromised, the imposter is cryptographically indistinguishable. Detection becomes behavioral:

| Signal | Meaning |
|--------|---------|
| Dual presence | Two nodes online with same identity |
| Location jump | Node appears from unexpected IP/region |
| Behavioral anomaly | Calling unusual endpoints |
| Heartbeat gap | Real node goes dark, imposter appears |

Mesh can enforce: reject connections from identity X at address Z if already connected from address Y.

## Permissions

Permissions control what a node can do. Uses pattern matching with namespaces.

**Format:**
```
permission = namespace:pattern
```

**Namespaces:**

| Namespace | Guards |
|-----------|--------|
| `endpoint:*` | RPC calls |
| `secret:*` | Vault secrets |
| `asset:*` | Cargobay files |
| `queue:*` | Flux queues |
| `flag:*` | Lever feature flags |
| `config:*` | Cluster configuration |
| `mesh:invite` | Inviting new nodes |
| `mesh:kick` | Removing nodes |
| `deploy:*` | Cortex deployments |

**Examples:**

| Node | Permissions | Can do |
|------|-------------|--------|
| `web-gateway` | `["endpoint:auth.*", "endpoint:users.*"]` | Call auth and user services |
| `rogue` | `["endpoint:_manage.*"]` | Call management endpoints |
| `bridge` | `["endpoint:*", "deploy:*"]` | Everything + deployments |
| `auth-service` | `["secret:DB_*", "secret:JWT_SECRET"]` | Read specific secrets |

## Permission Grants

Human admin is the root of trust. Permissions are granted via signed certificates.

**Grant structure:**
```rust
struct PermissionGrant {
    node_public_key: PublicKey,
    permissions: Vec<Pattern>,
    issued_at: Timestamp,
    expires_at: Option<Timestamp>,
    signature: Signature,  // admin's signature
}
```

**Flow:**
1. Admin has their own keypair (root of trust)
2. Admin creates grant: `{node_public_key, permissions, timestamps}`
3. Admin signs grant with their private key
4. Node stores grant alongside its private key
5. On mesh join, node presents public key + signed grant
6. Mesh verifies admin signature + binding to node's key
7. Node joins with granted permissions

**Verification on join:**
1. Grant signature valid (signed by admin)
2. Grant's `node_public_key` matches presenting node
3. Grant hasn't expired

## Vault (Secrets)

Secrets service with permission-based access and TTL caching.

**Flow:**
```
1. Node calls vault.get("STRIPE_API_KEY")
2. Vault checks: does caller have "secret:STRIPE_API_KEY" or "secret:STRIPE_*"?
3. If yes → return { value, ttl }
4. Node caches locally until TTL expires
```

**Secret definition:**
```yaml
secrets:
  - name: STRIPE_API_KEY
    value: sk_live_...
    ttl: 5m           # high sensitivity

  - name: DB_PASSWORD
    value: ...
    ttl: 1h           # medium sensitivity
```

## Cryptographic Primitives

All provided by the `shields` library:

| Primitive | Use |
|-----------|-----|
| Ed25519 | Signatures |
| X25519 | Key exchange |
| ChaCha20-Poly1305 / AES-GCM | Symmetric encryption |
| BLAKE3 / SHA256 | Hashing |
| HKDF | Key derivation |

Consider: Noise Protocol framework for session establishment.

## Integration with Node Framework

Identity is built into `libs/node`, not a separate service.

**Address book extended:**
```rust
struct PeerInfo {
    node_id: NodeId,
    service_name: String,
    public_key: PublicKey,
    permissions: Vec<Pattern>,
    addresses: Vec<...>,
}
```

**Transparent to developers:**
```rust
// Developer writes:
rpc.call("auth.login", request).await

// Framework handles:
// - Sign request
// - Encrypt (if enabled)
// - Verify response signature
// - Decrypt response
```

## Open Questions

### Key Rotation
How does a node change its keypair without losing identity? Options:
- Admin issues new grant for new key
- Node signs rotation request with old key, admin approves
- Grace period where both keys are valid

### Revocation
Compromised key or grant - how to invalidate?
- Admin-signed revocation list
- Short grant TTLs (force periodic renewal)
- Leader broadcasts revocation, replicated via Raft

### Admin Management
- Multiple admins?
- Hierarchy (root admin can delegate)?
- Admin key rotation?

### Grant Renewal
- Automatic renewal before expiry?
- Re-request from admin?
- Grace period for expired grants?

### Audit
- Logging who called what (probably telemetry's job)
- Tracking permission checks
- Recording grant issuance/revocation
