//! NodeBuilder for constructing nodes

use crate::binding::{Binding, ListenerHandle, ListenerWrapper};
use crate::error::{Error, Result};
use crate::handler::Handler;
use crate::node::{Data, Node};
use crate::scheduler::{OverlapPolicy, Schedule, ScheduledTaskConfig, Scheduler};
use constellation_fabric::transport::{Transport, TransportListener};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::{watch, RwLock};

use super::runtime::StartableNode;

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
