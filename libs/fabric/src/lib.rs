//! Constellation Fabric - Low-level transport and codec layer
//!
//! Provides transport abstractions (TCP, Unix sockets) and codec support
//! (bincode, raw bytes) for service-to-service communication.
//!
//! # Example
//!
//! ```no_run
//! use constellation_fabric::{Channel, codec::BincodeCodec, request::request_tcp};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct MyRequest { data: String }
//!
//! #[derive(Serialize, Deserialize)]
//! struct MyResponse { result: i32 }
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // One-off request
//! let addr = "127.0.0.1:8080".parse()?;
//! let req = MyRequest { data: "hello".to_string() };
//! let resp: MyResponse = request_tcp(addr, &req, BincodeCodec).await?;
//!
//! // Or use a persistent channel
//! let mut channel = Channel::tcp(addr, BincodeCodec).await?;
//! channel.send(&req).await?;
//! let resp: MyResponse = channel.receive().await?;
//! # Ok(())
//! # }
//! ```

pub mod channel;
pub mod codec;
pub mod error;
pub mod request;
pub mod transport;

// Re-exports for convenience
pub use channel::Channel;
pub use error::{Error, Result};
