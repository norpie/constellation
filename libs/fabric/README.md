# Constellation Fabric

Low-level transport and codec layer for service-to-service communication.

## What it provides

- `Transport` trait - Extensible connections (TCP, Unix sockets, custom)
- `TransportListener` trait - Accept incoming connections
- `Codec` trait - Pluggable serialization (bincode, protobuf, custom)
- `Channel` - High-level typed message passing
- Builder pattern with timeout support
- Length-prefix framing for message boundaries

## Example

```rust
use constellation_fabric::{Channel, codec::BincodeCodec};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct Message { data: String }

// Client
let mut channel = Channel::tcp("127.0.0.1:8080".parse()?, BincodeCodec).await?;
channel.send(&Message { data: "hello".to_string() }).await?;
let response: Message = channel.receive().await?;
```
