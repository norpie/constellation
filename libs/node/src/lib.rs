pub mod codec;
pub mod error;
pub mod handler;
pub mod rpc;

// Re-export the derive macro
pub use constellation_node_derive::handler;

// Re-export commonly used types
pub use codec::{BincodeFactory, CodecFactory, RawCodecFactory, TypedCodec};
pub use error::{Error, Result};

// Placeholder Node struct
pub struct Node {
    // TODO: implement
}
