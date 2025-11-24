// Node structure and builder

use crate::error::{Error, Result};
use crate::handler::Handler;
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
    data: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    routes: HashMap<String, &'static dyn Handler>,
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
}

/// Builder for constructing a Node
pub struct NodeBuilder {
    service_name: Option<String>,
    data: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    routes: HashMap<String, &'static dyn Handler>,
}

impl NodeBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            service_name: None,
            data: HashMap::new(),
            routes: HashMap::new(),
        }
    }

    /// Set the service name (required)
    ///
    /// This name will be prepended to all handler routes.
    pub fn service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = Some(name.into());
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

    /// Build the Node
    ///
    /// This will:
    /// - Validate service name is set
    /// - Prepend service name to all routes
    /// - Register built-in handlers
    /// - Auto-register RpcClient as Data<RpcClient>
    pub fn build(self) -> Result<Node> {
        let service_name = self
            .service_name
            .ok_or_else(|| Error::Custom("Service name is required".to_string()))?;

        // Prepend service name to all user routes
        let mut routes = HashMap::new();
        for (route, handler) in self.routes {
            let full_route = format!("{}.{}", service_name, route);
            routes.insert(full_route, handler);
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
            data,
            routes,
        })
    }
}

impl Default for NodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}
