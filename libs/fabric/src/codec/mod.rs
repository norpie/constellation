use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Codec for serializing and deserializing messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Codec {
    /// Binary serialization via bincode (default)
    #[default]
    Bincode,
    // Future: Json, Protobuf, MessagePack, etc.
}

impl Codec {
    /// Return the name of this codec
    pub fn name(&self) -> &'static str {
        match self {
            Self::Bincode => "bincode",
        }
    }

    /// Encode a value into bytes
    pub fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>> {
        match self {
            Self::Bincode => bincode::serialize(value).map_err(|e| Error::Codec(e.to_string())),
        }
    }

    /// Decode bytes into a value
    pub fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T> {
        match self {
            Self::Bincode => bincode::deserialize(bytes).map_err(|e| Error::Codec(e.to_string())),
        }
    }
}
