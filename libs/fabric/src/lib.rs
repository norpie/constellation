//! Constellation Fabric - Low-level transport and codec layer
//!
//! Provides transport abstractions (TCP, Unix sockets) and codec support
//! for service-to-service communication.
//!
//! # Example
//!
//! ```no_run
//! use constellation_fabric::{Channel, Codec};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct MyRequest { data: String }
//!
//! #[derive(Serialize, Deserialize)]
//! struct MyResponse { result: i32 }
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let addr = "127.0.0.1:8080".parse()?;
//! let req = MyRequest { data: "hello".to_string() };
//!
//! let mut channel = Channel::tcp(addr, Codec::Bincode).await?;
//! channel.send(&req).await?;
//! let resp: MyResponse = channel.receive().await?;
//! # Ok(())
//! # }
//! ```

pub mod channel;
pub mod codec;
pub mod error;
pub mod transport;

// Re-exports for convenience
pub use channel::Channel;
pub use codec::Codec;
pub use error::{Error, Result};
