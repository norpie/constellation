// Node structure and builder

use crate::error::{Error, Result};
use crate::handler::Handler;
use constellation_fabric::transport::{Transport, TransportListener};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

/// Wrapper for shared application data
///
/// Data is cheaply cloneable (Arc internally) and can be extracted
/// in handlers via the extractor pattern.
pub struct Data<T>(Arc<T>);

impl<T> Data<T> {
    /// Create new Data wrapper
    pub fn new(value: T) -> Self {
        Self(Arc::new(value))
    }
}

impl<T> Clone for Data<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> Deref for Data<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

/// Node represents a service in the mesh
pub struct Node {
    service_name: String,
    node_id: Option<String>,
    voting_member: bool,
    id_fallback: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    data: Arc<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
    routes: Arc<HashMap<String, &'static dyn Handler>>,
    listeners: Vec<(Box<dyn ListenerHandle>, String)>,
}

impl Node {
    /// Create a new NodeBuilder
    pub fn builder() -> NodeBuilder {
        NodeBuilder::new()
    }

    /// Extract shared data by type
    pub fn extract<T: 'static>(&self) -> Option<Data<T>> {
        self.data
            .get(&TypeId::of::<Data<T>>())
            .and_then(|any| any.downcast_ref::<Data<T>>())
            .cloned()
    }

    /// Get the service name
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// Get the node ID (if set)
    pub fn node_id(&self) -> Option<&str> {
        self.node_id.as_deref()
    }

    /// Check if this node is configured as a voting member
    pub fn is_voting_member(&self) -> bool {
        self.voting_member
    }

    /// Get the ID fallback strategy (if set)
    pub fn id_fallback(&self) -> Option<&Arc<dyn Fn(String) -> String + Send + Sync>> {
        self.id_fallback.as_ref()
    }

    /// Start the node runtime
    ///
    /// Spawns listener tasks for all configured transports and begins accepting
    /// incoming RPC requests. This method runs until a shutdown signal is received
    /// (Ctrl+C).
    ///
    /// # Example
    /// ```ignore
    /// let node = Node::builder()
    ///     .service_name("MyService")
    ///     .listen(tcp_listener, "default")
    ///     .build()?;
    ///
    /// node.start().await?; // Runs forever
    /// ```
    pub async fn start(mut self) -> Result<()> {
        // Extract listeners before wrapping in Arc
        let listeners = std::mem::take(&mut self.listeners);
        let node = Arc::new(self);

        // Spawn a task for each listener
        for (listener, zone) in listeners {
            let node_clone = Arc::clone(&node);

            tokio::spawn(async move {
                loop {
                    match listener.accept_connection().await {
                        Ok(transport) => {
                            let node = Arc::clone(&node_clone);
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(transport, node).await {
                                    eprintln!("Connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("Accept error on zone {}: {}", zone, e);
                            break;
                        }
                    }
                }
            });
        }

        // Wait for shutdown signal
        tokio::signal::ctrl_c()
            .await
            .map_err(|e| Error::Custom(format!("Failed to listen for shutdown signal: {}", e)))?;

        Ok(())
    }
}

/// Handle a single connection - receive requests, dispatch to handlers, send responses
async fn handle_connection(mut transport: Box<dyn Transport>, node: Arc<Node>) -> Result<()> {
    loop {
        // Receive frame from transport
        let frame = transport.receive().await?;

        // Parse RPC frame (our custom format with separate header/payload)
        let (header, payload) = crate::rpc::parse_frame(&frame)?;

        // Lookup handler for this route
        let handler = node
            .routes
            .get(&header.route)
            .ok_or_else(|| Error::RouteNotFound(header.route.clone()))?;

        // Build RpcRequest for handler
        let request = crate::rpc::RpcRequest {
            request_id: header.request_id,
            route: header.route.clone(),
            payload: payload.to_vec(),
        };

        // Execute handler and build ResponseResult
        let result = match handler.call(&node, &request).await {
            Ok(success_payload) => crate::rpc::ResponseResult::Success(success_payload),
            Err(handler_err) => {
                eprintln!(
                    "Handler error for route {} (category: {:?})",
                    header.route, handler_err.category
                );
                crate::rpc::ResponseResult::Error {
                    category: handler_err.category,
                    payload: handler_err.payload,
                }
            }
        };

        // Wrap in RpcResponse
        let response = crate::rpc::RpcResponse {
            request_id: header.request_id,
            result,
        };

        // Serialize entire RpcResponse
        let codec = constellation_fabric::codec::BincodeCodec;
        let response_payload = constellation_fabric::codec::Codec::encode(&codec, &response)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        // Build response frame
        let response_header = crate::rpc::RpcHeader {
            request_id: header.request_id,
            route: header.route,
        };
        let response_frame = crate::rpc::pack_frame(&response_header, &response_payload)?;

        // Send response
        transport.send(&response_frame).await?;
    }
}

/// Object-safe wrapper for TransportListener (internal use)
///
/// Allows storing heterogeneous listeners in NodeBuilder
#[async_trait::async_trait]
trait ListenerHandle: Send + Sync {
    /// Accept a connection and return a boxed Transport
    async fn accept_connection(&self) -> Result<Box<dyn Transport>>;
}

/// Wrapper that implements ListenerHandle for any TransportListener
struct ListenerWrapper<L>(L);

#[async_trait::async_trait]
impl<L> ListenerHandle for ListenerWrapper<L>
where
    L: TransportListener + Send + Sync,
    L::Transport: Transport + Send + Sync + 'static,
{
    async fn accept_connection(&self) -> Result<Box<dyn Transport>> {
        let transport = self.0.accept().await?;
        Ok(Box::new(transport))
    }
}

/// Builder for constructing a Node
pub struct NodeBuilder {
    service_name: Option<String>,
    node_id: Option<String>,
    voting_member: bool,
    id_fallback: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    data: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    routes: HashMap<String, &'static dyn Handler>,
    auto_discover: bool,
    listeners: Vec<(Box<dyn ListenerHandle>, String)>, // (listener, zone)
}

impl NodeBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            service_name: None,
            node_id: None,
            voting_member: true, // Default to voting member per spec
            id_fallback: None,
            data: HashMap::new(),
            routes: HashMap::new(),
            auto_discover: true,
            listeners: Vec::new(),
        }
    }

    /// Set the service name (required)
    ///
    /// This name will be prepended to all handler routes.
    pub fn service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = Some(name.into());
        self
    }

    /// Set the node ID
    ///
    /// Node ID must be unique across the mesh. If not set, a random ID will be generated.
    /// On ID conflict during join, the node will attempt to use the fallback strategy if configured.
    pub fn id(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }

    /// Set the ID fallback strategy
    ///
    /// If the node's ID conflicts with another node in the mesh during join,
    /// this function will be called with the original ID to generate a new one.
    ///
    /// # Example
    /// ```ignore
    /// Node::builder()
    ///     .id("service-node")
    ///     .id_fallback(|original_id| {
    ///         format!("{}-{}", original_id, uuid::Uuid::new_v4())
    ///     })
    ///     .build()
    /// ```
    pub fn id_fallback<F>(mut self, fallback: F) -> Self
    where
        F: Fn(String) -> String + Send + Sync + 'static,
    {
        self.id_fallback = Some(Arc::new(fallback));
        self
    }

    /// Set whether this node joins as a voting member
    ///
    /// - `true` (default): Node participates in Raft elections and counts toward quorum
    /// - `false`: Node is an observer - receives replication but doesn't vote
    pub fn voting_member(mut self, voting: bool) -> Self {
        self.voting_member = voting;
        self
    }

    /// Register shared application data
    ///
    /// This data can be extracted in handlers via Data<T> extractor.
    pub fn data<T: 'static + Send + Sync>(mut self, value: T) -> Self {
        let data = Data::new(value);
        self.data.insert(TypeId::of::<Data<T>>(), Box::new(data));
        self
    }

    /// Manually register a handler
    ///
    /// The route will be prepended with the service name during build.
    pub fn register(mut self, route: impl Into<String>, handler: &'static dyn Handler) -> Self {
        self.routes.insert(route.into(), handler);
        self
    }

    /// Add a listener for any transport type
    ///
    /// This method is fully extensible - it works with any implementation of
    /// `TransportListener`, including custom user-defined transports.
    ///
    /// # Arguments
    /// * `listener` - Any type implementing `TransportListener`
    /// * `zone` - Network zone identifier (e.g., "default", "internal", "dc-east")
    ///
    /// # Example
    /// ```ignore
    /// use constellation_fabric::transport::{TcpTransportListener, UnixTransportListener};
    ///
    /// let tcp = TcpTransportListener::bind("127.0.0.1:8080".parse()?).await?;
    /// let unix = UnixTransportListener::bind("/tmp/service.sock").await?;
    ///
    /// Node::builder()
    ///     .service_name("MyService")
    ///     .listen(tcp, "default")
    ///     .listen(unix, "local")
    ///     .build()?
    ///     .start().await?;
    /// ```
    pub fn listen<L>(mut self, listener: L, zone: impl Into<String>) -> Self
    where
        L: TransportListener + Send + Sync + 'static,
        L::Transport: Transport + Send + Sync + 'static,
    {
        let wrapped = ListenerWrapper(listener);
        self.listeners.push((Box::new(wrapped), zone.into()));
        self
    }

    /// Enable or disable automatic handler discovery via inventory
    ///
    /// When enabled (default), all handlers marked with #[handler] are automatically
    /// registered at build time. Disable this for tests where you want manual control
    /// over which handlers are registered.
    ///
    /// # Example
    /// ```ignore
    /// // Production: auto-discover all handlers
    /// Node::builder()
    ///     .service_name("MyService")
    ///     .build();
    ///
    /// // Tests: manual registration only
    /// Node::builder()
    ///     .service_name("MyService")
    ///     .auto_discover(false)
    ///     .register("method.v1", &MY_HANDLER)
    ///     .build();
    /// ```
    pub fn auto_discover(mut self, enable: bool) -> Self {
        self.auto_discover = enable;
        self
    }

    /// Build the Node
    ///
    /// This will:
    /// - Validate service name is set
    /// - Auto-discover handlers via inventory (if enabled)
    /// - Prepend service name to all routes
    /// - Register built-in handlers
    /// - Auto-register RpcClient as Data<RpcClient>
    pub fn build(self) -> Result<Node> {
        let service_name = self
            .service_name
            .ok_or_else(|| Error::Custom("Service name is required".to_string()))?;

        // Start with manually registered routes
        let mut routes = HashMap::new();
        for (route, handler) in self.routes {
            let full_route = format!("{}.{}", service_name, route);
            routes.insert(full_route, handler);
        }

        // Auto-discover handlers via inventory if enabled
        if self.auto_discover {
            for registration in inventory::iter::<crate::handler::HandlerRegistration> {
                let route = format!(
                    "{}.{}.v{}",
                    service_name, registration.method, registration.version
                );

                // Skip if already manually registered (manual takes precedence)
                if !routes.contains_key(&route) {
                    routes.insert(route, registration.handler);
                }
            }
        }

        // Auto-register RpcClient
        let mut data = self.data;
        let rpc_client = crate::rpc::RpcClient::new();
        data.insert(
            TypeId::of::<Data<crate::rpc::RpcClient>>(),
            Box::new(Data::new(rpc_client)),
        );

        // TODO: Register built-in handlers (_mesh.*, _raft.*, etc.)

        Ok(Node {
            service_name,
            node_id: self.node_id,
            voting_member: self.voting_member,
            id_fallback: self.id_fallback,
            data: Arc::new(data),
            routes: Arc::new(routes),
            listeners: self.listeners,
        })
    }
}

impl Default for NodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}
