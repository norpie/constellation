// Handler trait and registration mechanism

use crate::error::Result;

/// Trait implemented by all RPC handlers
#[async_trait::async_trait]
pub trait Handler: Send + Sync {
    /// Handle an RPC request
    ///
    /// The raw bytes from the request will be decoded by the handler
    /// and the response will be encoded back to bytes
    async fn call(
        &self,
        node: &crate::Node,
        request: &crate::rpc::RpcRequest,
    ) -> Result<Vec<u8>>;
}

/// Handler registration for inventory discovery
pub struct HandlerRegistration {
    pub method: &'static str,
    pub version: u32,
    pub handler: &'static dyn Handler,
}

inventory::collect!(HandlerRegistration);

impl HandlerRegistration {
    pub const fn new(method: &'static str, version: u32, handler: &'static dyn Handler) -> Self {
        Self {
            method,
            version,
            handler,
        }
    }
}
