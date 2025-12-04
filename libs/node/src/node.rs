// Node structure and builder

use crate::error::{Error, Result};
use crate::handler::Handler;
use crate::scheduler::{
    OverlapPolicy, Schedule, ScheduledTaskConfig, Scheduler, SchedulerCommand,
};
use constellation_fabric::transport::{Transport, TransportListener};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::{mpsc, watch, RwLock};

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
    node_id: String,
    region: String,
    zone: String,
    can_lead: bool,
    global_constraints: crate::mesh::Constraint,
    id_fallback: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    data: Arc<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
    routes: Arc<HashMap<String, &'static dyn Handler>>,
    listeners: Vec<(Box<dyn ListenerHandle>, String)>,
    advertise_addresses: Vec<crate::mesh::AddressGroup>,
    raft: constellation_raft::RaftNode<crate::mesh::AddressBook>,
    // Scheduler fields
    scheduler_rx: Option<mpsc::Receiver<SchedulerCommand>>,
    scheduler_tx: mpsc::Sender<SchedulerCommand>,
    shutdown_tx: watch::Sender<bool>,
    initial_tasks: Vec<ScheduledTaskConfig>,
}

/// A node ready to be started
///
/// This is a temporary wrapper returned by `NodeBuilder::build()` that holds
/// the bootstrap configuration separately from the Node. When `start()` is called,
/// the bootstrap peers are consumed and the Node is wrapped in an Arc for the runtime.
pub struct StartableNode {
    node: Node,
    bootstrap_peers: Vec<(String, crate::mesh::AddressGroup)>,
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

    /// Get the node ID
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// Check if this node can become a leader
    pub fn can_lead(&self) -> bool {
        self.can_lead
    }

    /// Get the region
    pub fn region(&self) -> &str {
        &self.region
    }

    /// Get the zone
    pub fn zone(&self) -> &str {
        &self.zone
    }

    /// Get the global constraints
    pub fn global_constraints(&self) -> &crate::mesh::Constraint {
        &self.global_constraints
    }

    /// Get the ID fallback strategy (if set)
    pub fn id_fallback(&self) -> Option<&Arc<dyn Fn(String) -> String + Send + Sync>> {
        self.id_fallback.as_ref()
    }

    /// Shutdown the node gracefully
    ///
    /// Signals all background tasks (scheduler, listeners) to stop.
    /// Tasks will complete their current work before stopping.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl StartableNode {
    /// Get a reference to the inner Node
    pub fn node(&self) -> &Node {
        &self.node
    }

    /// Extract shared data by type (delegates to inner Node)
    pub fn extract<T: 'static>(&self) -> Option<Data<T>> {
        self.node.extract()
    }

    /// Get the service name (delegates to inner Node)
    pub fn service_name(&self) -> &str {
        self.node.service_name()
    }

    /// Get the node ID (delegates to inner Node)
    pub fn node_id(&self) -> &str {
        self.node.node_id()
    }

    /// Check if this node can become a leader (delegates to inner Node)
    pub fn can_lead(&self) -> bool {
        self.node.can_lead()
    }

    /// Get the region (delegates to inner Node)
    pub fn region(&self) -> &str {
        self.node.region()
    }

    /// Get the zone (delegates to inner Node)
    pub fn zone(&self) -> &str {
        self.node.zone()
    }

    /// Get the global constraints (delegates to inner Node)
    pub fn global_constraints(&self) -> &crate::mesh::Constraint {
        self.node.global_constraints()
    }

    /// Get the ID fallback strategy (delegates to inner Node)
    pub fn id_fallback(&self) -> Option<&Arc<dyn Fn(String) -> String + Send + Sync>> {
        self.node.id_fallback()
    }

    /// Start the node runtime
    ///
    /// Spawns listener tasks for all configured transports, joins the cluster
    /// (or forms a new one), starts Raft consensus tasks, and begins accepting
    /// incoming RPC requests.
    ///
    /// Returns an `Arc<Node>` that can be used to interact with the running node.
    /// Call `node.shutdown()` to gracefully stop all background tasks.
    ///
    /// # Startup Sequence
    /// 1. Start transport listeners (accept connections)
    /// 2. Start scheduler loop (user tasks only)
    /// 3. Bootstrap: join existing cluster or form new one
    /// 4. Start Raft tasks (election, heartbeat, apply)
    ///
    /// # Example
    /// ```ignore
    /// let node = Node::builder()
    ///     .service_name("MyService")
    ///     .listen(tcp_listener, "default", "tcp", "192.168.1.10:8080")
    ///     .build()?;
    ///
    /// let running_node = node.start().await?;
    /// // ... do work ...
    /// running_node.shutdown();
    /// ```
    pub async fn start(self) -> Result<Arc<Node>> {
        // Extract bootstrap_peers (will be dropped after use)
        let bootstrap_peers = self.bootstrap_peers;
        let mut node = self.node;

        // Extract fields before wrapping in Arc
        let listeners = std::mem::take(&mut node.listeners);
        let scheduler_rx = node.scheduler_rx.take();
        let initial_tasks = std::mem::take(&mut node.initial_tasks);
        let advertise_addresses = std::mem::take(&mut node.advertise_addresses);
        let data = Arc::clone(&node.data);
        let routes = Arc::clone(&node.routes);
        let shutdown_tx = node.shutdown_tx.clone();
        let scheduler_tx = node.scheduler_tx.clone();
        let node_id = node.node_id.clone();
        let region = node.region.clone();
        let zone = node.zone.clone();
        let can_lead = node.can_lead;
        let global_constraints = node.global_constraints.clone();
        let raft = node.raft.clone();

        let node = Arc::new(node);

        // 1. Spawn listener tasks (accept connections)
        for (listener, zone) in listeners {
            let node_clone = Arc::clone(&node);
            let mut shutdown_rx = shutdown_tx.subscribe();

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        result = listener.accept_connection() => {
                            match result {
                                Ok(transport) => {
                                    let node = Arc::clone(&node_clone);
                                    tokio::spawn(async move {
                                        if let Err(_e) = handle_connection(transport, node).await {
                                            // eprintln!("Connection error: {}", _e);
                                        }
                                    });
                                }
                                Err(e) => {
                                    eprintln!("Accept error on zone {}: {}", zone, e);
                                    break;
                                }
                            }
                        }
                        _ = shutdown_rx.changed() => {
                            if *shutdown_rx.borrow() {
                                break;
                            }
                        }
                    }
                }
            });
        }

        // 2. Spawn scheduler loop (but don't schedule Raft tasks yet)
        if let Some(scheduler_rx) = scheduler_rx {
            let shutdown_rx = shutdown_tx.subscribe();
            let scheduler_tx_clone = scheduler_tx.clone();

            tokio::spawn(async move {
                crate::scheduler::run_scheduler_loop(
                    scheduler_rx,
                    data,
                    shutdown_rx,
                    scheduler_tx_clone,
                )
                .await;
            });

            // Register initial user tasks (from builder)
            for task_config in initial_tasks {
                let id = crate::scheduler::TaskId::new();
                let (response_tx, _response_rx) = tokio::sync::oneshot::channel();

                let _ = scheduler_tx
                    .send(SchedulerCommand::Schedule {
                        id,
                        name: task_config.name,
                        schedule: task_config.schedule,
                        policy: task_config.policy,
                        task: task_config.task,
                        response: response_tx,
                    })
                    .await;
            }
        }

        // 3. Bootstrap: join cluster or form new one
        let self_data = build_self_transponder_data(&node_id, &region, &zone, &advertise_addresses, &routes, &global_constraints);
        bootstrap_join(&bootstrap_peers, &self_data, &raft, can_lead).await?;

        // bootstrap_peers is dropped here (local variable goes out of scope after use)

        // 4. NOW schedule Raft tasks (we're part of a cluster)
        let scheduler = Scheduler::from_sender(scheduler_tx.clone());
        let raft_config = match node.extract::<RwLock<crate::config::Config>>() {
            Some(cfg) => cfg.read().await.raft.clone(),
            None => crate::config::RaftConfig::default(),
        };
        if let Err(e) = crate::raft_tasks::schedule_raft_tasks(&scheduler, &raft_config).await {
            eprintln!("Warning: Failed to schedule Raft tasks: {}", e);
        }

        // 5. Return the running node
        // Caller can call node.shutdown() when they want to stop
        Ok(node)
    }
}

/// Handle a single connection - receive requests, dispatch to handlers, send responses
async fn handle_connection(mut transport: Box<dyn Transport>, node: Arc<Node>) -> Result<()> {
    // println!("[handle_connection] New connection on node {}", node.node_id);
    loop {
        // Receive frame from transport
        let frame = transport.receive().await?;
        // println!("[handle_connection] Received frame ({} bytes)", frame.len());

        // Parse RPC frame (our custom format with separate header/payload)
        let (header, payload) = crate::rpc::parse_frame(&frame)?;
        // println!("[handle_connection] Route: {}", header.route);

        // Lookup handler for this route
        let handler = node
            .routes
            .get(&header.route)
            .ok_or_else(|| {
                println!("[handle_connection] Route not found: {}", header.route);
                println!("[handle_connection] Available routes: {:?}", node.routes.keys().collect::<Vec<_>>());
                Error::RouteNotFound(header.route.clone())
            })?;

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
        let response_payload = constellation_fabric::Codec::Bincode
            .encode(&response)
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

// ListenerHandle and ListenerWrapper are now in binding.rs
use crate::binding::{Binding, ListenerHandle, ListenerWrapper};

/// Builder for constructing a Node
pub struct NodeBuilder {
    service_name: Option<String>,
    node_id: Option<String>,
    region: Option<String>,
    zone: Option<String>,
    can_lead: bool,
    id_fallback: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    data: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    routes: HashMap<String, &'static dyn Handler>,
    auto_discover: bool,
    listeners: Vec<(Box<dyn ListenerHandle>, String)>, // (listener, zone)
    advertise_addresses: Vec<crate::mesh::AdvertisedAddress>, // addresses to advertise in TransponderData
    bootstrap_peers: Vec<(String, crate::mesh::AdvertisedAddress)>, // (node_id, address)
    scheduled_tasks: Vec<ScheduledTaskConfig>,
    global_constraints: Option<crate::mesh::Constraint>,
}

impl NodeBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            service_name: None,
            node_id: None,
            region: None,
            zone: None,
            can_lead: true, // Default to allowing leadership
            id_fallback: None,
            data: HashMap::new(),
            routes: HashMap::new(),
            auto_discover: true,
            listeners: Vec::new(),
            advertise_addresses: Vec::new(),
            bootstrap_peers: Vec::new(),
            scheduled_tasks: Vec::new(),
            global_constraints: None,
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

    /// Set the geographic region for this node
    ///
    /// Used for topology-aware routing to prefer nodes in the same region.
    /// Defaults to "global" if not set.
    ///
    /// # Example
    /// ```ignore
    /// Node::builder()
    ///     .service_name("MyService")
    ///     .region("us-east")
    ///     .zone("us-east-1a")
    ///     .build()
    /// ```
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Set the availability zone for this node
    ///
    /// Used for topology-aware routing to prefer nodes in the same zone.
    /// Defaults to "global" if not set.
    pub fn zone(mut self, zone: impl Into<String>) -> Self {
        self.zone = Some(zone.into());
        self
    }

    /// Set global constraints for this node
    ///
    /// Global constraints apply to all connections to/from this node.
    /// Individual routes can override these with route-specific constraints.
    ///
    /// # Example
    /// ```ignore
    /// use constellation_node::mesh::{Constraint, ConnectionRules};
    /// use constellation_fabric::Codec;
    ///
    /// Node::builder()
    ///     .service_name("MyService")
    ///     .global_constraints(
    ///         Constraint::allow_all()
    ///             .with_network("external", ConnectionRules::only("tls", Codec::Bincode))
    ///     )
    ///     .build()
    /// ```
    pub fn global_constraints(mut self, constraints: crate::mesh::Constraint) -> Self {
        self.global_constraints = Some(constraints);
        self
    }

    /// Set whether this node can become a leader
    ///
    /// - `true` (default): Node can start elections and become leader
    /// - `false`: Node never starts elections (remains follower), but still votes and counts toward quorum
    pub fn can_lead(mut self, can_lead: bool) -> Self {
        self.can_lead = can_lead;
        self
    }

    /// Add a bootstrap peer for initial cluster formation
    ///
    /// Bootstrap peers are used to initially discover and join the Raft cluster.
    /// Once the cluster is formed, the address book (replicated via Raft) is used
    /// for all peer communication.
    ///
    /// # Example
    /// ```ignore
    /// Node::builder()
    ///     .service_name("MyService")
    ///     .id("node-1")
    ///     .with_peer("node-2", AddressGroup::single("default", "tcp", "10.0.1.2:8080"))
    ///     .with_peer("node-3", AddressGroup::single("default", "tcp", "10.0.1.3:8080"))
    ///     .build()
    /// ```
    pub fn with_peer(
        mut self,
        node_id: impl Into<String>,
        address_group: crate::mesh::AddressGroup,
    ) -> Self {
        self.bootstrap_peers.push((node_id.into(), address_group));
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
    /// If a handler is already registered for this route, the new one is skipped.
    pub fn register(mut self, route: impl Into<String>, handler: &'static dyn Handler) -> Self {
        let route = route.into();
        if self.routes.contains_key(&route) {
            eprintln!(
                "Warning: Duplicate handler registration for route '{}', skipping",
                route
            );
        } else {
            self.routes.insert(route, handler);
        }
        self
    }

    /// Add a binding (listener + codecs + advertised addresses)
    ///
    /// This is the preferred way to configure listeners. Each binding ties together
    /// a transport listener with the codecs it supports and the addresses it advertises.
    ///
    /// # Example
    /// ```ignore
    /// use constellation_fabric::transport::TcpTransportListener;
    /// use constellation_fabric::Codec;
    /// use constellation_node::Binding;
    ///
    /// let listener = TcpTransportListener::bind("0.0.0.0:8080".parse()?).await?;
    ///
    /// let binding = Binding::new(listener, "tcp")
    ///     .codecs([Codec::Bincode])
    ///     .advertise("internal", "10.0.1.5:8080")
    ///     .advertise("external", "203.0.113.5:8080");
    ///
    /// Node::builder()
    ///     .service_name("MyService")
    ///     .binding(binding)
    ///     .build()?
    ///     .start().await?;
    /// ```
    pub fn binding(mut self, binding: Binding) -> Self {
        // Use first network as the zone for logging (will be refactored with AdvertisedAddress)
        let zone = binding
            .advertised()
            .first()
            .map(|e| e.network.clone())
            .unwrap_or_else(|| "default".to_string());

        // Convert AdvertisedEndpoints to AdvertisedAddresses
        for endpoint in binding.advertised() {
            self.advertise_addresses.push(crate::mesh::AdvertisedAddress {
                network: endpoint.network.clone(),
                transport: binding.transport().to_string(),
                address: endpoint.address.clone(),
                codecs: binding.codecs_list().to_vec(),
                binding_id: binding.binding_id().to_string(),
            });
        }

        // Store listener
        self.listeners.push((binding.listener, zone));
        self
    }

    /// Add a listener for any transport type
    ///
    /// **Deprecated:** Use [`binding()`](Self::binding) instead, which provides
    /// better support for codecs and multi-network advertisement.
    ///
    /// # Arguments
    /// * `listener` - Any type implementing `TransportListener`
    /// * `zone` - Network zone identifier (e.g., "default", "internal", "dc-east")
    /// * `transport_name` - Transport protocol name (e.g., "tcp", "unix")
    /// * `advertise_address` - Address to advertise to other nodes (e.g., "192.168.1.10:8080")
    #[deprecated(since = "0.2.0", note = "Use `binding()` instead")]
    pub fn listen<L>(
        mut self,
        listener: L,
        zone: impl Into<String>,
        transport_name: impl Into<String>,
        advertise_address: impl Into<String>,
    ) -> Self
    where
        L: TransportListener + Send + Sync + 'static,
        L::Transport: Transport + Send + Sync + 'static,
    {
        let zone = zone.into();
        let transport = transport_name.into();
        let address = advertise_address.into();

        // Store advertise address for TransponderData
        self.advertise_addresses.push(crate::mesh::AdvertisedAddress {
            network: zone.clone(),
            transport,
            address,
            codecs: vec![constellation_fabric::Codec::Bincode],
            binding_id: String::new(),
        });

        // Wrap and store listener
        let wrapped = ListenerWrapper(listener);
        self.listeners.push((Box::new(wrapped), zone));
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

    /// Schedule a task to run according to the given schedule
    ///
    /// Tasks are executed when `Node::start()` is called. Tasks have access to
    /// the same extractors as handlers (Data<T>, etc.).
    ///
    /// # Example
    /// ```ignore
    /// use std::time::Duration;
    ///
    /// Node::builder()
    ///     .service_name("MyService")
    ///     .schedule(Schedule::every(Duration::from_secs(60)), |ctx| async move {
    ///         let db: Data<DbPool> = ctx.extract().unwrap();
    ///         db.cleanup().await;
    ///     })
    ///     .build()
    /// ```
    pub fn schedule<F, Fut>(mut self, schedule: Schedule, task: F) -> Self
    where
        F: Fn(crate::scheduler::TaskContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.scheduled_tasks.push(ScheduledTaskConfig {
            name: None,
            schedule,
            policy: OverlapPolicy::default(),
            task: Arc::new(task),
        });
        self
    }

    /// Schedule a named task
    ///
    /// Named tasks are easier to identify when listing or debugging.
    pub fn schedule_named<F, Fut>(
        mut self,
        name: impl Into<String>,
        schedule: Schedule,
        task: F,
    ) -> Self
    where
        F: Fn(crate::scheduler::TaskContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.scheduled_tasks.push(ScheduledTaskConfig {
            name: Some(name.into()),
            schedule,
            policy: OverlapPolicy::default(),
            task: Arc::new(task),
        });
        self
    }

    /// Schedule a task with custom overlap policy
    pub fn schedule_with_policy<F, Fut>(
        mut self,
        schedule: Schedule,
        policy: OverlapPolicy,
        task: F,
    ) -> Self
    where
        F: Fn(crate::scheduler::TaskContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.scheduled_tasks.push(ScheduledTaskConfig {
            name: None,
            schedule,
            policy,
            task: Arc::new(task),
        });
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
    ///
    /// Returns a `StartableNode` which can be started with `.start().await`.
    pub fn build(self) -> Result<StartableNode> {
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
                // Use custom route if specified, otherwise construct from service name + method + version
                let route = if let Some(custom_route) = registration.route {
                    custom_route.to_string()
                } else {
                    format!(
                        "{}.{}.v{}",
                        service_name, registration.method, registration.version
                    )
                };

                // Skip if already registered (manual or earlier auto-discovered takes precedence)
                if routes.contains_key(&route) {
                    eprintln!(
                        "Warning: Duplicate handler for route '{}' during auto-discovery, skipping",
                        route
                    );
                } else {
                    routes.insert(route, registration.handler);
                }
            }
        }

        // Determine node ID: use provided or generate from service_name + uuid
        let node_id = self
            .node_id
            .unwrap_or_else(|| format!("{}_{}", service_name, uuid::Uuid::new_v4()));

        // Extract peer IDs for Raft initialization
        let peer_ids: Vec<String> = self
            .bootstrap_peers
            .iter()
            .map(|(id, _)| id.clone())
            .collect();

        // Create Config first (other components need values from it)
        let config_values = crate::config::Config::default();
        let scheduler_buffer_size = config_values.scheduler.channel_buffer_size;

        // Create Raft node with peer IDs and config
        let raft = constellation_raft::RaftNode::builder()
            .node_id(node_id.clone())
            .can_lead(self.can_lead)
            .peers(peer_ids)
            .config(config_values.raft_crate.clone())
            .storage(constellation_raft::MemoryStorage::new())
            .state_machine(crate::mesh::AddressBook::new())
            .build()?;
        let config = Data::new(RwLock::new(config_values));

        // Create Router (needs node_id and raft)
        let router = crate::router::Router::new(node_id.clone(), raft.clone());

        // Create RpcClient (needs router and live config reference)
        let rpc_client = crate::rpc::RpcClient::new(router.clone(), Data::clone(&config));

        // Auto-register components
        let mut data = self.data;

        data.insert(
            TypeId::of::<Data<crate::router::Router>>(),
            Box::new(Data::new(router)),
        );

        data.insert(
            TypeId::of::<Data<crate::rpc::RpcClient>>(),
            Box::new(Data::new(rpc_client)),
        );

        data.insert(
            TypeId::of::<Data<constellation_raft::RaftNode<crate::mesh::AddressBook>>>(),
            Box::new(Data::new(raft.clone())),
        );

        // Create Scheduler and auto-register
        // Note: buffer size is read at creation time and can't be changed at runtime
        let (scheduler, scheduler_rx) = Scheduler::new(scheduler_buffer_size);
        let scheduler_tx = scheduler.command_tx();
        data.insert(
            TypeId::of::<Data<Scheduler>>(),
            Box::new(Data::new(scheduler)),
        );

        // Register Config (already created above for RpcClient)
        data.insert(
            TypeId::of::<Data<RwLock<crate::config::Config>>>(),
            Box::new(config),
        );

        // Create shutdown channel
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);

        // TODO: Register built-in handlers (_mesh.*, _raft.*, etc.)

        let node = Node {
            service_name,
            node_id,
            region: self.region.unwrap_or_else(|| "global".to_string()),
            zone: self.zone.unwrap_or_else(|| "global".to_string()),
            can_lead: self.can_lead,
            global_constraints: self.global_constraints.unwrap_or_default(),
            id_fallback: self.id_fallback,
            data: Arc::new(data),
            routes: Arc::new(routes),
            listeners: self.listeners,
            advertise_addresses: self.advertise_addresses,
            raft,
            scheduler_rx: Some(scheduler_rx),
            scheduler_tx,
            shutdown_tx,
            initial_tasks: self.scheduled_tasks,
        };

        Ok(StartableNode {
            node,
            bootstrap_peers: self.bootstrap_peers,
        })
    }
}

impl Default for NodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Build TransponderData for this node
fn build_self_transponder_data(
    node_id: &str,
    region: &str,
    zone: &str,
    advertise_addresses: &[crate::mesh::AddressGroup],
    routes: &HashMap<String, &'static dyn crate::handler::Handler>,
    global_constraints: &crate::mesh::Constraint,
) -> crate::mesh::TransponderData {
    let route_names: Vec<String> = routes.keys().cloned().collect();

    // Collect unique transports
    let transports: Vec<String> = advertise_addresses
        .iter()
        .map(|a| a.transport.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    crate::mesh::TransponderData::builder()
        .node_id(node_id)
        .region(region)
        .zone(zone)
        .addresses(advertise_addresses.to_vec())
        .transports(transports)
        .codec("bincode") // Currently hardcoded
        .routes(route_names)
        .global_constraints(global_constraints.clone())
        .capabilities(crate::mesh::Capabilities::basic())
        .build()
}

/// Attempt to join the cluster via bootstrap peers
///
/// Returns Ok(()) if successfully joined or formed new cluster.
/// Returns Err if unable to join and unable to form new cluster.
async fn bootstrap_join(
    bootstrap_peers: &[(String, crate::mesh::AddressGroup)],
    self_data: &crate::mesh::TransponderData,
    raft: &constellation_raft::RaftNode<crate::mesh::AddressBook>,
    can_lead: bool,
) -> Result<()> {
    println!("[bootstrap_join] Node {} starting bootstrap (peers: {})", self_data.node_id, bootstrap_peers.len());

    // If no bootstrap peers, we're forming a new cluster
    if bootstrap_peers.is_empty() {
        println!("[bootstrap_join] No bootstrap peers - forming new cluster");
        if !can_lead {
            return Err(Error::Custom(
                "Cannot start: no bootstrap peers and can_lead=false".to_string(),
            ));
        }
        // First node: become leader first, then add ourselves via the log
        // This ensures our Join entry is in the log and will be replicated to joiners.
        raft.start_election().await?;
        println!("[bootstrap_join] First node became leader");

        // Now submit our Join command through the log
        let command = crate::mesh::AddressBookCommand::Join(self_data.clone());
        let bytes = constellation_fabric::Codec::Bincode
            .encode(&command)
            .map_err(|e| Error::Custom(format!("Failed to serialize join command: {}", e)))?;
        raft.submit_command(bytes).await?;
        println!("[bootstrap_join] First node added self to AddressBook");
        return Ok(());
    }

    // Try bootstrap peers sequentially
    for (peer_id, advertised) in bootstrap_peers {
        let address = &advertised.address;

        println!("[bootstrap_join] Trying to join via peer {} at {}", peer_id, address);

        // Attempt join
        match try_join(address, self_data).await {
            Ok(crate::mesh::MeshResponse::Success) => {
                println!("[bootstrap_join] Successfully joined via {}", address);
                return Ok(());
            }
            Ok(crate::mesh::MeshResponse::NotLeader {
                leader: Some(leader_data),
            }) => {
                println!("[bootstrap_join] Peer {} is not leader, redirecting to {:?}", address, leader_data.node_id);
                // Got redirected to leader, try that
                if let Some(leader_addr) = leader_data.addresses.first().map(|a| &a.address) {
                    println!("[bootstrap_join] Trying leader at {}", leader_addr);
                    if let Ok(crate::mesh::MeshResponse::Success) =
                        try_join(leader_addr, self_data).await
                    {
                        println!("[bootstrap_join] Successfully joined via leader");
                        return Ok(());
                    }
                }
            }
            Ok(crate::mesh::MeshResponse::NotLeader { leader: None }) => {
                println!("[bootstrap_join] Peer {} has no leader info, trying next", address);
                continue;
            }
            Err(e) => {
                println!("[bootstrap_join] Connection to {} failed: {}", address, e);
                continue;
            }
        }
    }

    println!("[bootstrap_join] Failed to join via any bootstrap peer");
    Err(Error::Custom(
        "Failed to join cluster via any bootstrap peer".to_string(),
    ))
}

async fn try_join(
    address: &str,
    self_data: &crate::mesh::TransponderData,
) -> Result<crate::mesh::MeshResponse> {
    println!("[try_join] Sending _mesh.join to {}", address);
    let result = crate::rpc::send_direct(address, "_mesh.join", self_data).await;
    println!("[try_join] Result: {:?}", result.as_ref().map(|_| "ok").map_err(|e| e.to_string()));
    result
}
