//! Node runtime - startup and connection handling

use crate::error::{Error, Result};
use crate::node::Node;
use crate::scheduler::{Scheduler, SchedulerCommand, TaskId};
use crate::telemetry::{BufferCollector, Span, TelemetryContext, TraceId};
use crate::Data;
use crate::rpc::DEFAULT_CODEC;
use constellation_fabric::channel::Channel;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::bootstrap::{bootstrap_join, build_self_transponder_data};

/// A node ready to be started
///
/// This is a temporary wrapper returned by `NodeBuilder::build()` that holds
/// the bootstrap configuration separately from the Node. When `start()` is called,
/// the bootstrap peers are consumed and the Node is wrapped in an Arc for the runtime.
pub struct StartableNode {
    pub(crate) node: Node,
    pub(crate) bootstrap_peers: Vec<(String, crate::mesh::AddressGroup)>,
}

impl StartableNode {
    /// Get a reference to the inner Node
    pub fn node(&self) -> &Node {
        &self.node
    }

    /// Extract shared data by type (delegates to inner Node)
    pub fn extract<T: 'static>(&self) -> Option<super::Data<T>> {
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
    /// ```text
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
                let id = TaskId::new();
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
        if let Err(e) = crate::raft::schedule_raft_tasks(&scheduler, &raft_config).await {
            eprintln!("Warning: Failed to schedule Raft tasks: {}", e);
        }

        // 5. Mark health registry as started
        if let Some(registry) = node.extract::<crate::health::HealthRegistry>() {
            registry.mark_started();
        }

        // 6. Return the running node
        // Caller can call node.shutdown() when they want to stop
        Ok(node)
    }
}

/// Handle a single connection - receive requests, dispatch to handlers, send responses
async fn handle_connection(transport: Box<dyn constellation_fabric::transport::Transport>, node: Arc<Node>) -> Result<()> {
    // Wrap transport in a Channel with default codec
    let mut channel = Channel::from_transport_boxed(transport, DEFAULT_CODEC);

    // Try to get collector for telemetry (if telemetry is enabled)
    let collector: Option<Data<BufferCollector>> = node.extract();

    loop {
        // Receive framed message (header + payload)
        let (header, payload): (crate::rpc::RpcHeader, Vec<u8>) =
            channel.receive_framed().await?;

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
            payload,
        };

        // Execute handler, optionally wrapped in telemetry context
        let result = if let Some(ref collector) = collector {
            // Create telemetry context from incoming trace (or generate fresh)
            let trace_id = header.trace_id.clone().unwrap_or_else(TraceId::new);
            let ctx = TelemetryContext::from_request(
                node.service_name(),
                node.node_id(),
                trace_id,
                header.parent_span_id.clone(),
                collector.clone(),
            );

            // Execute handler within telemetry context
            let node_clone = node.clone();
            let route = header.route.clone();
            ctx.scope(async move {
                // Create span for handler execution
                let span_name = format!("rpc.server/{}", route);
                let mut span_guard = Span::enter(&span_name);
                span_guard.set_tag("rpc.route", &route);

                let result = match handler.call(&node_clone, &request).await {
                    Ok(success_payload) => crate::rpc::ResponseResult::Success(success_payload),
                    Err(handler_err) => {
                        span_guard.set_error();
                        span_guard.set_tag("rpc.error_category", format!("{:?}", handler_err.category));
                        eprintln!(
                            "Handler error for route {} (category: {:?})",
                            route, handler_err.category
                        );
                        crate::rpc::ResponseResult::Error {
                            category: handler_err.category,
                            payload: handler_err.payload,
                        }
                    }
                };
                result
            }).await
        } else {
            // No telemetry - execute handler directly
            match handler.call(&node, &request).await {
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
            }
        };

        // Wrap in RpcResponse
        let response = crate::rpc::RpcResponse {
            request_id: header.request_id,
            result,
        };

        // Serialize RpcResponse as payload
        let response_payload = channel
            .codec()
            .encode(&response)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        // Build response header (propagate trace context from request)
        let response_header = crate::rpc::RpcHeader {
            request_id: header.request_id,
            route: header.route,
            trace_id: header.trace_id,
            parent_span_id: header.parent_span_id,
        };

        // Send framed response
        channel.send_framed(&response_header, &response_payload).await?;
    }
}
