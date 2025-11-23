// Codec factory system for object-safe codec dispatch

use crate::error::Result;
use constellation_fabric::codec::Codec;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

/// Object-safe trait for type-specific codec operations
///
/// This trait works around the limitation that the generic Codec trait
/// cannot be used as a trait object (dyn Codec is not allowed).
pub trait TypedCodec<Req, Resp>: Send + Sync {
    /// Decode request bytes into the request type
    fn decode_request(&self, bytes: &[u8]) -> Result<Req>;

    /// Encode response into bytes
    fn encode_response(&self, response: &Resp) -> Result<Vec<u8>>;
}

/// Factory for creating typed codec instances
///
/// Implementations should wrap a concrete Codec type and create
/// TypedCodec instances for specific request/response type pairs.
pub trait CodecFactory: Send + Sync {
    /// Create a typed codec for the given request/response types
    fn create_typed<Req, Resp>(&self) -> Box<dyn TypedCodec<Req, Resp>>
    where
        Req: for<'de> Deserialize<'de> + Send + Sync + 'static,
        Resp: Serialize + Send + Sync + 'static;
}

/// Adapter that wraps any Codec into a TypedCodec
struct CodecAdapter<C, Req, Resp> {
    codec: C,
    _phantom: PhantomData<(Req, Resp)>,
}

impl<C, Req, Resp> TypedCodec<Req, Resp> for CodecAdapter<C, Req, Resp>
where
    C: Codec,
    Req: for<'de> Deserialize<'de> + Send + Sync + 'static,
    Resp: Serialize + Send + Sync + 'static,
{
    fn decode_request(&self, bytes: &[u8]) -> Result<Req> {
        self.codec
            .decode(bytes)
            .map_err(|e| crate::Error::Serialization(e.to_string()))
    }

    fn encode_response(&self, response: &Resp) -> Result<Vec<u8>> {
        self.codec
            .encode(response)
            .map_err(|e| crate::Error::Serialization(e.to_string()))
    }
}

/// Factory for BincodeCodec
pub struct BincodeFactory;

impl CodecFactory for BincodeFactory {
    fn create_typed<Req, Resp>(&self) -> Box<dyn TypedCodec<Req, Resp>>
    where
        Req: for<'de> Deserialize<'de> + Send + Sync + 'static,
        Resp: Serialize + Send + Sync + 'static,
    {
        Box::new(CodecAdapter {
            codec: constellation_fabric::codec::BincodeCodec,
            _phantom: PhantomData,
        })
    }
}

/// Factory for RawCodec
pub struct RawCodecFactory;

impl CodecFactory for RawCodecFactory {
    fn create_typed<Req, Resp>(&self) -> Box<dyn TypedCodec<Req, Resp>>
    where
        Req: for<'de> Deserialize<'de> + Send + Sync + 'static,
        Resp: Serialize + Send + Sync + 'static,
    {
        Box::new(CodecAdapter {
            codec: constellation_fabric::codec::RawCodec,
            _phantom: PhantomData,
        })
    }
}
