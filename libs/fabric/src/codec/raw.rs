use serde::{Deserialize, Serialize};

use crate::codec::Codec;
use crate::error::{Error, Result};

/// Raw codec that passes through bytes without serialization
///
/// Only works with Vec<u8> and &[u8]
#[derive(Debug, Clone, Copy, Default)]
pub struct RawCodec;

impl Codec for RawCodec {
    fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>> {
        // This is a bit hacky but works for Vec<u8>
        bincode::serialize(value).map_err(|e| Error::Codec(e.to_string()))
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T> {
        bincode::deserialize(bytes).map_err(|e| Error::Codec(e.to_string()))
    }
}

// Helper functions for raw bytes
impl RawCodec {
    pub fn encode_bytes(&self, bytes: &[u8]) -> Vec<u8> {
        bytes.to_vec()
    }

    pub fn decode_bytes(&self, bytes: &[u8]) -> Vec<u8> {
        bytes.to_vec()
    }
}
