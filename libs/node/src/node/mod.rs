//! Node structure and core types

mod bootstrap;
mod builder;
mod runtime;

pub use builder::NodeBuilder;
pub use runtime::StartableNode;

use crate::handler::Handler;
use crate::scheduler::SchedulerCommand;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use crate::binding::ListenerHandle;

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
    pub(crate) service_name: String,
    pub(crate) node_id: String,
    pub(crate) region: String,
    pub(crate) zone: String,
    pub(crate) can_lead: bool,
    pub(crate) global_constraints: crate::mesh::Constraint,
    pub(crate) id_fallback: Option<Arc<dyn Fn(String) -> String + Send + Sync>>,
    pub(crate) data: Arc<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
    pub(crate) routes: Arc<HashMap<String, &'static dyn Handler>>,
    pub(crate) listeners: Vec<(Box<dyn ListenerHandle>, String)>,
    pub(crate) advertise_addresses: Vec<crate::mesh::AddressGroup>,
    pub(crate) raft: constellation_raft::RaftNode<crate::mesh::AddressBook>,
    // Scheduler fields
    pub(crate) scheduler_rx: Option<mpsc::Receiver<SchedulerCommand>>,
    pub(crate) scheduler_tx: mpsc::Sender<SchedulerCommand>,
    pub(crate) shutdown_tx: watch::Sender<bool>,
    pub(crate) initial_tasks: Vec<crate::scheduler::ScheduledTaskConfig>,
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
