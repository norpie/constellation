use serde::{Deserialize, Serialize};

use crate::error::Result;

pub mod bincode;
pub mod raw;

pub use self::bincode::BincodeCodec;
pub use self::raw::RawCodec;

/// Codec trait for serializing and deserializing messages
pub trait Codec: Send + Sync {
    /// Encode a value into bytes
    fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>>;

    /// Decode bytes into a value
    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T>;
}
