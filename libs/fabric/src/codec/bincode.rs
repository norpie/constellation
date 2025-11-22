use serde::{Deserialize, Serialize};

use crate::codec::Codec;
use crate::error::{Error, Result};

/// Bincode codec for binary serialization
#[derive(Debug, Clone, Copy, Default)]
pub struct BincodeCodec;

impl Codec for BincodeCodec {
    fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>> {
        bincode::serialize(value).map_err(|e| Error::Codec(e.to_string()))
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T> {
        bincode::deserialize(bytes).map_err(|e| Error::Codec(e.to_string()))
    }
}
