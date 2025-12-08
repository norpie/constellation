// Handler trait and registration mechanism

/// Trait implemented by all RPC handlers
#[async_trait::async_trait]
pub trait Handler: Send + Sync {
    /// Handle an RPC request
    ///
    /// Handlers decode the request payload and encode the response using the provided codec.
    /// Returns either serialized success bytes or a HandlerError with category + error payload.
    async fn call(
        &self,
        node: &crate::Node,
        request: &crate::rpc::RpcRequest,
        codec: &constellation_fabric::Codec,
    ) -> std::result::Result<Vec<u8>, crate::rpc::HandlerError>;
}

/// Handler registration for inventory discovery
pub struct HandlerRegistration {
    pub method: &'static str,
    pub version: u32,
    pub route: Option<&'static str>,
    pub handler: &'static dyn Handler,
}

inventory::collect!(HandlerRegistration);

impl HandlerRegistration {
    pub const fn new(method: &'static str, version: u32, handler: &'static dyn Handler) -> Self {
        Self {
            method,
            version,
            route: None,
            handler,
        }
    }

    pub const fn with_route(method: &'static str, version: u32, route: &'static str, handler: &'static dyn Handler) -> Self {
        Self {
            method,
            version,
            route: Some(route),
            handler,
        }
    }
}
