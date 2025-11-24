// RPC request/response types
//
// ## Wire Protocol
//
// To avoid double-serialization overhead, we use a custom frame format:
//
// ```
// [header_len: u32][header_bytes: ...][payload_bytes: ...]
// ```
//
// Where:
// - `header_len`: 4-byte big-endian u32 indicating length of header_bytes
// - `header_bytes`: Serialized RpcHeader (request_id + route)
// - `payload_bytes`: Already-serialized user request payload
//
// This avoids the overhead of serializing Vec<u8> within RpcRequest.
// Channel provides outer framing; this is inner framing for efficiency.

use dashmap::DashMap;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub request_id: Uuid,
    pub route: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub request_id: Uuid,
    pub result: ResponseResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseResult {
    Success(Vec<u8>),
    Error {
        category: ErrorCategory,
        payload: Vec<u8>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ErrorCategory {
    Retryable,
    ServerError,
    ClientError,
    Timeout,
    Unavailable,
}

/// RPC header containing metadata (for wire protocol efficiency)
///
/// This is serialized separately from the payload to avoid nested Vec<u8> serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RpcHeader {
    request_id: Uuid,
    route: String,
}

/// Pack an RPC frame for transmission
///
/// Creates a frame with format: [header_len: u32][header_bytes][payload_bytes]
/// This avoids double-serialization of the payload.
fn pack_frame(header: &RpcHeader, payload: &[u8]) -> crate::Result<Vec<u8>> {
    // Serialize header
    let codec = constellation_fabric::codec::BincodeCodec;
    let header_bytes = constellation_fabric::codec::Codec::encode(&codec, header)
        .map_err(|e| crate::Error::Serialization(e.to_string()))?;

    // Calculate total frame size
    let header_len = header_bytes.len() as u32;
    let total_len = 4 + header_bytes.len() + payload.len();

    // Build frame
    let mut frame = Vec::with_capacity(total_len);
    frame.extend_from_slice(&header_len.to_be_bytes());
    frame.extend_from_slice(&header_bytes);
    frame.extend_from_slice(payload);

    Ok(frame)
}

/// Parse an RPC frame received from the wire
///
/// Returns the deserialized header and a reference to the payload bytes.
fn parse_frame(frame: &[u8]) -> crate::Result<(RpcHeader, &[u8])> {
    // Read header length
    if frame.len() < 4 {
        return Err(crate::Error::Custom("Frame too short".to_string()));
    }

    let header_len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;

    // Validate frame length
    if frame.len() < 4 + header_len {
        return Err(crate::Error::Custom("Incomplete frame".to_string()));
    }

    // Deserialize header
    let header_bytes = &frame[4..4 + header_len];
    let codec = constellation_fabric::codec::BincodeCodec;
    let header: RpcHeader = constellation_fabric::codec::Codec::decode(&codec, header_bytes)
        .map_err(|e| crate::Error::Serialization(e.to_string()))?;

    // Extract payload
    let payload = &frame[4 + header_len..];

    Ok((header, payload))
}

/// Client for making outbound RPC calls
///
/// RpcClient is automatically registered as Data<RpcClient> in every node
/// and can be extracted in handlers for making calls to other services.
pub struct RpcClient {
    /// Per-route round-robin state for load balancing
    rr_state: Arc<DashMap<String, AtomicUsize>>,

    // Future: address book reference for service discovery
    // address_book: Arc<RwLock<AddressBook>>,

    // Future: resiliency configuration
    // config: Arc<ResiliencyConfig>,
}

impl RpcClient {
    /// Create a new RpcClient (internal use only)
    pub(crate) fn new() -> Self {
        Self {
            rr_state: Arc::new(DashMap::new()),
        }
    }

    /// Make an RPC call to another service
    ///
    /// Serializes the request immediately using bincode. Returns a builder for
    /// configuring retry, timeout, and backoff options.
    ///
    /// # Example
    /// ```ignore
    /// let response: UserData = rpc
    ///     .call("UsersService.get_user.v1", &GetUserRequest { id: 42 })?
    ///     .await?;
    /// ```
    pub fn call<Req, Resp>(&self, route: &str, request: &Req) -> crate::Result<RpcCallBuilder<Resp>>
    where
        Req: Serialize,
        Resp: DeserializeOwned,
    {
        // Serialize immediately - more efficient than cloning the request
        let codec = constellation_fabric::codec::BincodeCodec;
        let payload = constellation_fabric::codec::Codec::encode(&codec, request)
            .map_err(|e| crate::Error::Serialization(e.to_string()))?;

        Ok(RpcCallBuilder::new(self.clone(), route.to_string(), payload))
    }
}

impl Clone for RpcClient {
    fn clone(&self) -> Self {
        Self {
            rr_state: Arc::clone(&self.rr_state),
        }
    }
}

/// Builder for configuring per-call retry and timeout options
pub struct RpcCallBuilder<Resp> {
    client: RpcClient,
    route: String,
    payload: Vec<u8>,
    max_attempts: Option<u32>,
    timeout_per_attempt: Option<Duration>,
    total_timeout: Option<Duration>,
    // Future: backoff strategy
    // backoff: Option<BackoffStrategy>,
    _phantom: PhantomData<Resp>,
}

impl<Resp> RpcCallBuilder<Resp>
where
    Resp: DeserializeOwned,
{
    fn new(client: RpcClient, route: String, payload: Vec<u8>) -> Self {
        Self {
            client,
            route,
            payload,
            max_attempts: None,
            timeout_per_attempt: None,
            total_timeout: None,
            _phantom: PhantomData,
        }
    }

    /// Set maximum number of retry attempts
    pub fn max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = Some(n);
        self
    }

    /// Set timeout for each individual attempt
    pub fn timeout_per_attempt(mut self, duration: Duration) -> Self {
        self.timeout_per_attempt = Some(duration);
        self
    }

    /// Set total timeout across all attempts
    pub fn total_timeout(mut self, duration: Duration) -> Self {
        self.total_timeout = Some(duration);
        self
    }

    // Future: backoff strategy configuration
    // pub fn backoff(mut self, strategy: BackoffStrategy) -> Self { ... }
}

impl<Resp> std::future::IntoFuture for RpcCallBuilder<Resp>
where
    Resp: DeserializeOwned,
{
    type Output = crate::Result<Resp>;
    type IntoFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            // TODO: Implement actual RPC call logic
            //
            // Pseudocode for when runtime is implemented:
            //
            // 1. Lookup route in address book
            //    let nodes = address_book.get_nodes_for_route(&self.route)?;
            //
            // 2. Round-robin node selection
            //    let node_idx = self.client.rr_state
            //        .entry(self.route.clone())
            //        .or_insert(AtomicUsize::new(0))
            //        .fetch_add(1, Ordering::Relaxed) % nodes.len();
            //    let target_node = &nodes[node_idx];
            //
            // 3. Build RPC frame (efficient manual framing)
            //    let header = RpcHeader {
            //        request_id: Uuid::new_v4(),
            //        route: self.route.clone(),
            //    };
            //    let frame = pack_frame(&header, &self.payload)?;
            //
            // 4. Connect and send
            //    let channel = Channel::connect(target_node.address).await?;
            //    channel.send_raw(&frame).await?;
            //
            // 5. Receive and parse response
            //    let response_frame = channel.recv_raw().await?;
            //    let (response_header, response_payload) = parse_frame(&response_frame)?;
            //
            // 6. Deserialize response
            //    let codec = BincodeCodec;
            //    let response: Resp = codec.decode(response_payload)?;
            //    Ok(response)
            //
            // 7. Apply retry logic based on self.max_attempts, timeout_per_attempt, etc.

            // For now, return an error since runtime isn't implemented yet
            Err(crate::Error::Custom(
                "RPC runtime not yet implemented".to_string()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_parse_frame_roundtrip() {
        let header = RpcHeader {
            request_id: Uuid::new_v4(),
            route: "TestService.method.v1".to_string(),
        };
        let payload = b"test payload data";

        // Pack frame
        let frame = pack_frame(&header, payload).expect("pack_frame should succeed");

        // Verify frame structure
        assert!(frame.len() >= 4 + payload.len());
        let header_len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        assert_eq!(frame.len(), 4 + header_len + payload.len());

        // Parse frame
        let (parsed_header, parsed_payload) =
            parse_frame(&frame).expect("parse_frame should succeed");

        // Verify round-trip
        assert_eq!(parsed_header.request_id, header.request_id);
        assert_eq!(parsed_header.route, header.route);
        assert_eq!(parsed_payload, payload);
    }

    #[test]
    fn test_parse_frame_too_short() {
        let frame = vec![0, 0, 0]; // Only 3 bytes
        let result = parse_frame(&frame);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Frame too short"));
    }

    #[test]
    fn test_parse_frame_incomplete() {
        // Create a valid header but truncate the frame
        let header = RpcHeader {
            request_id: Uuid::new_v4(),
            route: "Test.method.v1".to_string(),
        };
        let payload = b"payload";

        let full_frame = pack_frame(&header, payload).unwrap();

        // Truncate to incomplete frame
        let incomplete = &full_frame[..full_frame.len() / 2];

        let result = parse_frame(incomplete);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Incomplete frame"));
    }

    #[test]
    fn test_frame_with_empty_payload() {
        let header = RpcHeader {
            request_id: Uuid::new_v4(),
            route: "Empty.test.v1".to_string(),
        };
        let payload = b"";

        let frame = pack_frame(&header, payload).unwrap();
        let (parsed_header, parsed_payload) = parse_frame(&frame).unwrap();

        assert_eq!(parsed_header.request_id, header.request_id);
        assert_eq!(parsed_header.route, header.route);
        assert_eq!(parsed_payload, payload);
    }

    #[test]
    fn test_frame_with_large_payload() {
        let header = RpcHeader {
            request_id: Uuid::new_v4(),
            route: "Large.payload.v1".to_string(),
        };
        let payload = vec![0xAB; 10000]; // 10KB payload

        let frame = pack_frame(&header, &payload).unwrap();
        let (parsed_header, parsed_payload) = parse_frame(&frame).unwrap();

        assert_eq!(parsed_header.request_id, header.request_id);
        assert_eq!(parsed_header.route, header.route);
        assert_eq!(parsed_payload, &payload[..]);
    }
}
