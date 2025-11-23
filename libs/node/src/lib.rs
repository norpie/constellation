pub mod codec;
pub mod error;
pub mod handler;
pub mod rpc;

mod node;

// Re-export the derive macro
pub use constellation_node_derive::handler;

// Re-export commonly used types
pub use codec::{BincodeFactory, CodecFactory, RawCodecFactory, TypedCodec};
pub use error::{Error, Result};
pub use node::{Data, Node, NodeBuilder};
