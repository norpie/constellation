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

use crate::config::Config;
use crate::Data;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Backoff strategy for retries
#[derive(Debug, Clone)]
pub enum BackoffStrategy {
    /// Fixed delay between retries
    Fixed(Duration),
    /// Exponential backoff: initial * 2^attempt, capped at max
    Exponential {
        initial: Duration,
        max: Duration,
    },
    /// No delay between retries
    None,
}

impl BackoffStrategy {
    /// Create default backoff strategy from Config
    pub fn from_config(config: &Config) -> Self {
        BackoffStrategy::Exponential {
            initial: Duration::from_millis(config.rpc.initial_backoff_ms),
            max: Duration::from_millis(config.rpc.max_backoff_ms),
        }
    }
}

impl Default for BackoffStrategy {
    fn default() -> Self {
        // Use Config defaults
        let config = Config::default();
        Self::from_config(&config)
    }
}

impl BackoffStrategy {
    /// Calculate delay for the given attempt number (0-indexed)
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        match self {
            BackoffStrategy::Fixed(d) => *d,
            BackoffStrategy::Exponential { initial, max } => {
                let multiplier = 2u32.saturating_pow(attempt);
                let delay = initial.saturating_mul(multiplier);
                delay.min(*max)
            }
            BackoffStrategy::None => Duration::ZERO,
        }
    }
}

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

/// Trait for successful responses
///
/// Any type that implements Serialize can be returned from a handler.
pub trait Responder: Serialize {}

// Blanket implementation - all Serialize types are valid responses
impl<T: Serialize> Responder for T {}

/// Trait for error responses that can be categorized for retry logic
///
/// Handler errors must implement this trait to provide an ErrorCategory
/// that determines retry behavior and circuit breaker logic.
pub trait ErrorResponder: Serialize {
    /// Return the error category for retry/circuit breaker logic
    fn error_category(&self) -> ErrorCategory;
}

/// Internal error type carrying both category and serialized error payload
///
/// This is used internally by the handler system. Users don't interact with this directly.
#[derive(Debug)]
pub struct HandlerError {
    pub category: ErrorCategory,
    pub payload: Vec<u8>,
}

/// RPC header containing metadata (for wire protocol efficiency)
///
/// This is serialized separately from the payload to avoid nested Vec<u8> serialization.
///
/// **Note**: This is an internal type exposed for testing. The API is unstable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[doc(hidden)]
pub struct RpcHeader {
    pub request_id: Uuid,
    pub route: String,
}

/// Pack an RPC frame for transmission
///
/// Creates a frame with format: [header_len: u32][header_bytes][payload_bytes]
/// This avoids double-serialization of the payload.
///
/// **Note**: This is an internal function exposed for testing. The API is unstable.
#[doc(hidden)]
pub fn pack_frame(header: &RpcHeader, payload: &[u8]) -> crate::Result<Vec<u8>> {
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
///
/// **Note**: This is an internal function exposed for testing. The API is unstable.
#[doc(hidden)]
pub fn parse_frame(frame: &[u8]) -> crate::Result<(RpcHeader, &[u8])> {
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
///
/// Uses Router for route resolution and load balancing.
pub struct RpcClient {
    /// Router for resolving routes and peers to connection targets
    router: crate::router::Router,
    /// Live config reference (reads current values at call time)
    config: Data<RwLock<Config>>,
}

impl RpcClient {
    /// Create a new RpcClient with a Router and config
    pub fn new(router: crate::router::Router, config: Data<RwLock<Config>>) -> Self {
        Self { router, config }
    }

    /// Make an RPC call to another service
    ///
    /// Resolves the route to a peer using round-robin load balancing,
    /// serializes the request using bincode, and returns a builder for
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

        Ok(RpcCallBuilder::new(
            self.clone(),
            route.to_string(),
            None, // No specific peer - use route resolution
            payload,
        ))
    }

    /// Make an RPC call to a specific peer
    ///
    /// Bypasses route-based load balancing and sends the request directly
    /// to the specified peer. Useful for Raft consensus messages and other
    /// peer-to-peer communication.
    ///
    /// # Example
    /// ```ignore
    /// let response: VoteResponse = rpc
    ///     .call_peer("node-2", "_raft.request_vote.v1", &vote_request)?
    ///     .await?;
    /// ```
    pub fn call_peer<Req, Resp>(
        &self,
        peer_id: &str,
        route: &str,
        request: &Req,
    ) -> crate::Result<RpcCallBuilder<Resp>>
    where
        Req: Serialize,
        Resp: DeserializeOwned,
    {
        // Serialize immediately
        let codec = constellation_fabric::codec::BincodeCodec;
        let payload = constellation_fabric::codec::Codec::encode(&codec, request)
            .map_err(|e| crate::Error::Serialization(e.to_string()))?;

        Ok(RpcCallBuilder::new(
            self.clone(),
            route.to_string(),
            Some(peer_id.to_string()), // Specific peer
            payload,
        ))
    }

    /// Get a reference to the underlying router
    pub fn router(&self) -> &crate::router::Router {
        &self.router
    }
}

impl Clone for RpcClient {
    fn clone(&self) -> Self {
        Self {
            router: self.router.clone(),
            config: Data::clone(&self.config),
        }
    }
}

/// Builder for configuring per-call retry and timeout options
///
/// Override fields are Option - None means "read from config at call time".
/// This ensures runtime config changes via management API take effect.
pub struct RpcCallBuilder<Resp> {
    client: RpcClient,
    route: String,
    /// Optional specific peer to call (None = use route resolution)
    peer_id: Option<String>,
    payload: Vec<u8>,
    /// Override: max retry attempts (None = use config)
    max_attempts: Option<u32>,
    /// Override: timeout per attempt (None = use config)
    timeout_per_attempt: Option<Duration>,
    /// Total timeout across all attempts (None = no limit)
    total_timeout: Option<Duration>,
    /// Override: backoff strategy (None = use config)
    backoff: Option<BackoffStrategy>,
    _phantom: PhantomData<Resp>,
}

impl<Resp> RpcCallBuilder<Resp>
where
    Resp: DeserializeOwned,
{
    fn new(client: RpcClient, route: String, peer_id: Option<String>, payload: Vec<u8>) -> Self {
        Self {
            client,
            route,
            peer_id,
            payload,
            max_attempts: None,
            timeout_per_attempt: None,
            total_timeout: None,
            backoff: None,
            _phantom: PhantomData,
        }
    }

    /// Set maximum number of retry attempts (overrides config)
    pub fn max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = Some(n);
        self
    }

    /// Disable retries (equivalent to max_attempts(1))
    pub fn no_retry(mut self) -> Self {
        self.max_attempts = Some(1);
        self
    }

    /// Set timeout for each individual attempt (overrides config)
    pub fn timeout_per_attempt(mut self, duration: Duration) -> Self {
        self.timeout_per_attempt = Some(duration);
        self
    }

    /// Set total timeout across all attempts (default: no limit)
    pub fn total_timeout(mut self, duration: Duration) -> Self {
        self.total_timeout = Some(duration);
        self
    }

    /// Set backoff strategy between retries (overrides config)
    pub fn backoff(mut self, strategy: BackoffStrategy) -> Self {
        self.backoff = Some(strategy);
        self
    }
}

/// Categorized error for retry decisions
enum AttemptError {
    /// Error is retryable (connection failed, timeout, server returned Retryable)
    Retryable(crate::Error),
    /// Error is not retryable (client error, serialization, etc.)
    Fatal(crate::Error),
}

impl<Resp> std::future::IntoFuture for RpcCallBuilder<Resp>
where
    Resp: DeserializeOwned,
{
    type Output = crate::Result<Resp>;
    type IntoFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            // Read config at call time (not at builder creation time)
            // This ensures runtime config changes take effect
            let config = self.client.config.read().await;
            let max_attempts = self.max_attempts.unwrap_or(config.rpc.max_attempts);
            let timeout_per_attempt = self
                .timeout_per_attempt
                .unwrap_or_else(|| Duration::from_millis(config.rpc.timeout_per_attempt_ms));
            let backoff = self
                .backoff
                .clone()
                .unwrap_or_else(|| BackoffStrategy::from_config(&config));
            drop(config); // Release lock before retry loop

            let start = Instant::now();
            let mut last_error: Option<crate::Error> = None;

            for attempt in 0..max_attempts {
                // Check total timeout before attempting
                if let Some(total) = self.total_timeout {
                    if start.elapsed() >= total {
                        return Err(last_error.unwrap_or_else(|| {
                            crate::Error::Timeout("Total timeout exceeded".to_string())
                        }));
                    }
                }

                // Apply backoff delay (skip on first attempt)
                if attempt > 0 {
                    let delay = backoff.delay_for_attempt(attempt - 1);
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                }

                // Execute attempt with timeout
                let attempt_future = Self::execute_attempt(
                    &self.client,
                    &self.route,
                    self.peer_id.as_deref(),
                    &self.payload,
                );

                let result = tokio::time::timeout(timeout_per_attempt, attempt_future).await;

                match result {
                    Ok(Ok(response)) => return Ok(response),
                    Ok(Err(AttemptError::Fatal(e))) => return Err(e),
                    Ok(Err(AttemptError::Retryable(e))) => {
                        last_error = Some(e);
                        // Continue to next attempt
                    }
                    Err(_timeout) => {
                        last_error = Some(crate::Error::Timeout(format!(
                            "Attempt {} timed out after {:?}",
                            attempt + 1,
                            timeout_per_attempt
                        )));
                        // Continue to next attempt
                    }
                }
            }

            // All attempts exhausted
            Err(last_error.unwrap_or_else(|| {
                crate::Error::Custom("All retry attempts failed".to_string())
            }))
        })
    }
}

impl<Resp> RpcCallBuilder<Resp>
where
    Resp: DeserializeOwned,
{
    /// Execute a single RPC attempt
    async fn execute_attempt(
        client: &RpcClient,
        route: &str,
        peer_id: Option<&str>,
        payload: &[u8],
    ) -> Result<Resp, AttemptError> {
        // 1. Resolve target using Router
        let target = match peer_id {
            Some(peer_id) => client
                .router
                .resolve_peer(peer_id)
                .await
                .map_err(|e| AttemptError::Fatal(e.into()))?,
            None => client
                .router
                .resolve_route(route)
                .await
                .map_err(|e| AttemptError::Fatal(e.into()))?,
        };

        // 2. Connect to target (connection errors are retryable)
        let addr: std::net::SocketAddr = target
            .address
            .parse()
            .map_err(|e| {
                AttemptError::Fatal(crate::Error::Custom(format!(
                    "Invalid address '{}': {}",
                    target.address, e
                )))
            })?;

        let mut transport = constellation_fabric::transport::TcpTransport::connect(addr)
            .await
            .map_err(|e| AttemptError::Retryable(e.into()))?;

        // 3. Build and send RPC frame
        let header = RpcHeader {
            request_id: Uuid::new_v4(),
            route: route.to_string(),
        };
        let frame = pack_frame(&header, payload).map_err(|e| AttemptError::Fatal(e))?;

        use constellation_fabric::transport::Transport;
        transport
            .send(&frame)
            .await
            .map_err(|e| AttemptError::Retryable(e.into()))?;

        // 4. Receive response (network errors are retryable)
        let response_frame = transport
            .receive()
            .await
            .map_err(|e| AttemptError::Retryable(e.into()))?;

        // 5. Parse response frame
        let (_response_header, response_payload) =
            parse_frame(&response_frame).map_err(|e| AttemptError::Fatal(e))?;

        // 6. Deserialize RpcResponse from payload
        let codec = constellation_fabric::codec::BincodeCodec;
        let response: RpcResponse =
            constellation_fabric::codec::Codec::decode(&codec, response_payload)
                .map_err(|e| AttemptError::Fatal(crate::Error::Serialization(e.to_string())))?;

        // 7. Handle response result
        match response.result {
            ResponseResult::Success(payload) => {
                let result: Resp = constellation_fabric::codec::Codec::decode(&codec, &payload)
                    .map_err(|e| AttemptError::Fatal(crate::Error::Serialization(e.to_string())))?;
                Ok(result)
            }
            ResponseResult::Error { category, payload } => {
                let error_msg: String =
                    constellation_fabric::codec::Codec::decode(&codec, &payload)
                        .unwrap_or_else(|_| format!("RPC error (category: {:?})", category));
                let error = crate::Error::Rpc(error_msg);

                // Determine if error is retryable based on category
                match category {
                    ErrorCategory::Retryable | ErrorCategory::Unavailable => {
                        Err(AttemptError::Retryable(error))
                    }
                    ErrorCategory::ClientError
                    | ErrorCategory::ServerError
                    | ErrorCategory::Timeout => Err(AttemptError::Fatal(error)),
                }
            }
        }
    }
}

/// Send an RPC request directly to an address (bypasses Router)
///
/// Used for bootstrap connections before AddressBook is populated.
/// This is a standalone function rather than a method on RpcClient because
/// it doesn't need any of RpcClient's infrastructure (Router, etc).
pub async fn send_direct<Req, Resp>(address: &str, route: &str, request: &Req) -> crate::Result<Resp>
where
    Req: serde::Serialize,
    Resp: DeserializeOwned,
{
    // Parse address
    let addr: std::net::SocketAddr = address
        .parse()
        .map_err(|e| crate::Error::Custom(format!("Invalid address '{}': {}", address, e)))?;

    // Connect directly
    let mut transport = constellation_fabric::transport::TcpTransport::connect(addr).await?;

    // Build and send frame
    let codec = constellation_fabric::codec::BincodeCodec;
    let payload = constellation_fabric::codec::Codec::encode(&codec, request)
        .map_err(|e| crate::Error::Serialization(e.to_string()))?;
    let header = RpcHeader {
        request_id: Uuid::new_v4(),
        route: route.to_string(),
    };
    let frame = pack_frame(&header, &payload)?;

    use constellation_fabric::transport::Transport;
    transport.send(&frame).await?;

    // Receive and parse response
    let response_frame = transport.receive().await?;
    let (_header, response_payload) = parse_frame(&response_frame)?;
    let response: RpcResponse = constellation_fabric::codec::Codec::decode(&codec, response_payload)
        .map_err(|e| crate::Error::Serialization(e.to_string()))?;

    // Handle result
    match response.result {
        ResponseResult::Success(payload) => {
            constellation_fabric::codec::Codec::decode(&codec, &payload)
                .map_err(|e| crate::Error::Serialization(e.to_string()))
        }
        ResponseResult::Error { category, payload } => {
            let error_msg: String =
                constellation_fabric::codec::Codec::decode(&codec, &payload)
                    .unwrap_or_else(|_| format!("RPC error (category: {:?})", category));
            Err(crate::Error::Rpc(error_msg))
        }
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
