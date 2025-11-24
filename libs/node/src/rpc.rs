// RPC request/response types

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
            // When implemented, we'll use:
            // - self.payload (already serialized, ready to send)
            // - self.route (for address book lookup)
            // - self.client.rr_state (for round-robin selection)
            // - self.max_attempts, timeout_per_attempt, etc. (for retry logic)

            // For now, return an error since runtime isn't implemented yet
            Err(crate::Error::Custom(
                "RPC runtime not yet implemented".to_string()
            ))
        })
    }
}
