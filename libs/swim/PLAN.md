# SWIM Protocol Implementation Plan

Gossip-based failure detection for Constellation, separate from Raft consensus.

## Why SWIM?

- **Raft** handles membership (who's in the cluster) — strong consistency, slower
- **SWIM** handles health (who's currently reachable) — eventual consistency, fast

Health data characteristics:
- Changes frequently (heartbeats, failures)
- Ephemeral (resets on restart)
- Eventual consistency is acceptable

## Protocol Overview

SWIM (Scalable Weakly-consistent Infection-style Process Group Membership) has two components:

### 1. Failure Detection

```
Every T seconds, node Mi picks random peer Mj:

1. DIRECT PROBE
   Mi ──PING──► Mj
   Mi ◄──ACK─── Mj  ✓ Done, Mj is alive

2. INDIRECT PROBE (if no ACK within timeout)
   Mi ──PING-REQ(Mj)──► k random nodes
   k nodes ──PING──► Mj ──ACK──► Mi  ✓ Done

3. SUSPICION (if still no ACK)
   Mark Mj as SUSPECTED (not dead yet!)
   Keep probing, give Mj a chance to respond
   Mj can also refute: "I'm alive!"

4. FAILURE (after suspicion timeout)
   Mark Mj as DEAD, disseminate via gossip
```

### 2. Dissemination

Instead of broadcasting failures separately, SWIM **piggybacks** membership updates on ping/ack messages. Every probe carries recent gossip ("btw, node-5 is suspected, node-3 joined").

## Key Properties

| Property | Mechanism |
|----------|-----------|
| Scalable | O(1) messages per node per period |
| Low false positives | Suspicion + indirect probes |
| Fast convergence | Infection-style gossip |
| Partition tolerant | Multiple paths to detect failure |

## Member States

```
     ┌─────────────────────────────────┐
     │                                 │
     ▼                                 │
  ┌──────┐    no response     ┌────────┴──┐    timeout    ┌──────┐
  │ Alive │ ───────────────► │ Suspected │ ────────────► │ Dead │
  └──────┘                    └───────────┘               └──────┘
     ▲                              │
     │         ack received         │
     └──────────────────────────────┘
```

## Crate Structure

```
constellation-swim/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── config.rs        # SwimConfig (T, k, timeouts)
│   ├── member.rs        # MemberState enum, Member struct
│   ├── detector.rs      # Failure detection (ping, ping-req, timeout)
│   ├── suspicion.rs     # Suspicion state machine with refutation
│   ├── disseminator.rs  # Piggyback gossip on messages
│   ├── message.rs       # Ping, Ack, PingReq, gossip payloads
│   └── swim.rs          # Main SwimNode coordinator
└── tests/
```

## Configuration Parameters

```rust
pub struct SwimConfig {
    /// Protocol period - how often to probe (T)
    pub probe_interval: Duration,     // e.g., 1 second

    /// Number of nodes for indirect probe (k)
    pub indirect_probe_count: usize,  // e.g., 3

    /// Timeout for direct probe before trying indirect
    pub probe_timeout: Duration,      // e.g., 500ms

    /// How long a node stays suspected before declared dead
    pub suspicion_timeout: Duration,  // e.g., 5 seconds

    /// Max gossip events to piggyback per message
    pub max_gossip_events: usize,     // e.g., 10
}
```

## Message Types

```rust
pub enum SwimMessage {
    Ping {
        sequence: u64,
        gossip: Vec<GossipEvent>,
    },
    Ack {
        sequence: u64,
        gossip: Vec<GossipEvent>,
    },
    PingReq {
        sequence: u64,
        target: NodeId,
        gossip: Vec<GossipEvent>,
    },
}

pub enum GossipEvent {
    Alive { node_id: NodeId, incarnation: u64 },
    Suspect { node_id: NodeId, incarnation: u64 },
    Dead { node_id: NodeId, incarnation: u64 },
}
```

## Integration with Node

```rust
// Node startup
let swim = SwimNode::new(node_id, config);

// Feed SWIM the membership from Raft AddressBook
swim.set_members(address_book.all_node_ids());

// Query health for routing decisions
let healthy_nodes = swim.healthy_nodes();
let is_healthy = swim.is_healthy(&node_id);

// When Raft membership changes, update SWIM
address_book.on_change(|event| {
    match event {
        MemberJoined(id) => swim.add_member(id),
        MemberLeft(id) => swim.remove_member(id),
    }
});

// Routing combines both
fn route_request(route: &str) -> Option<NodeId> {
    let candidates = address_book.get_nodes_for_route(route)?;
    let healthy: Vec<_> = candidates
        .iter()
        .filter(|id| swim.is_healthy(id))
        .collect();
    // Round-robin or random selection from healthy nodes
    select_node(&healthy)
}
```

## Implementation Phases

### Phase 1: Basic Detection
- [ ] Member struct and MemberState enum
- [ ] SwimConfig
- [ ] Direct ping/ack cycle
- [ ] Basic timeout → mark dead

### Phase 2: Indirect Probes
- [ ] PingReq message
- [ ] Probe through k random peers
- [ ] Forward acks back to originator

### Phase 3: Suspicion
- [ ] Suspect state (not immediate death)
- [ ] Suspicion timeout
- [ ] Incarnation numbers for crdered state
- [ ] Refutation (suspect can prove alive)

### Phase 4: Dissemination
- [ ] GossipEvent struct
- [ ] Piggyback on ping/ack messages
- [ ] Event queue with limited size
- [ ] Protocol-period bounded dissemination

### Phase 5: Integration
- [ ] SwimNode background task
- [ ] Integration with Node scheduler
- [ ] Health queries for RPC routing
- [ ] Tests with simulated network partitions

## References

- [Original SWIM Paper (Cornell)](https://www.cs.cornell.edu/projects/Quicksilver/public_pdfs/SWIM.pdf)
- [SWIM Protocol Explained](https://www.brianstorti.com/swim/)
- [HashiCorp Memberlist](https://github.com/hashicorp/memberlist)
- [Lifeguard Extensions](https://www.hashicorp.com/en/blog/making-gossip-more-robust-with-lifeguard)
- [Rust Memberlist (al8n)](https://github.com/al8n/memberlist)
