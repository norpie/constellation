use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Codec for serializing and deserializing messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Codec {
    /// Binary serialization via bincode (default) - fast, compact
    #[default]
    Bincode,
    /// JSON serialization - human-readable, widely compatible
    Json,
    /// MessagePack serialization - compact binary, faster than JSON
    MessagePack,
    /// CBOR serialization - binary, good for constrained environments
    Cbor,
    /// Postcard serialization - embedded-friendly, very compact
    Postcard,
}

impl Codec {
    /// Encode a value into bytes
    pub fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>> {
        match self {
            Self::Bincode => bincode::serialize(value).map_err(|e| Error::Codec(e.to_string())),
            Self::Json => serde_json::to_vec(value).map_err(|e| Error::Codec(e.to_string())),
            Self::MessagePack => {
                rmp_serde::to_vec(value).map_err(|e| Error::Codec(e.to_string()))
            }
            Self::Cbor => {
                let mut buf = Vec::new();
                ciborium::into_writer(value, &mut buf).map_err(|e| Error::Codec(e.to_string()))?;
                Ok(buf)
            }
            Self::Postcard => {
                postcard::to_allocvec(value).map_err(|e| Error::Codec(e.to_string()))
            }
        }
    }

    /// Decode bytes into a value
    pub fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T> {
        match self {
            Self::Bincode => bincode::deserialize(bytes).map_err(|e| Error::Codec(e.to_string())),
            Self::Json => serde_json::from_slice(bytes).map_err(|e| Error::Codec(e.to_string())),
            Self::MessagePack => {
                rmp_serde::from_slice(bytes).map_err(|e| Error::Codec(e.to_string()))
            }
            Self::Cbor => {
                ciborium::from_reader(bytes).map_err(|e| Error::Codec(e.to_string()))
            }
            Self::Postcard => {
                postcard::from_bytes(bytes).map_err(|e| Error::Codec(e.to_string()))
            }
        }
    }
}
