use crate::mesh::TransponderData;
use constellation_raft::{Result, StateMachine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Commands that can be applied to the address book
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AddressBookCommand {
    /// Add a node to the mesh
    Join(TransponderData),
    /// Remove a node from the mesh
    Leave(String),
    /// Update a node's transponder data
    Update(String, TransponderData),
}

/// Responses from address book operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AddressBookResponse {
    /// Operation succeeded
    Success,
    /// Node already exists
    AlreadyExists,
    /// Node not found
    NotFound,
}

/// The address book state machine
///
/// This is replicated via Raft consensus. All nodes maintain an identical
/// copy of the address book, which maps node IDs to their transponder data.
pub struct AddressBook {
    /// Map of node_id -> TransponderData
    nodes: HashMap<String, TransponderData>,
    /// Reverse index: route -> Vec<node_id>
    route_index: HashMap<String, Vec<String>>,
}

impl AddressBook {
    /// Create a new empty address book
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            route_index: HashMap::new(),
        }
    }

    /// Get transponder data for a node
    pub fn get_node(&self, node_id: &str) -> Option<&TransponderData> {
        self.nodes.get(node_id)
    }

    /// Get all node IDs that handle a specific route
    pub fn get_nodes_for_route(&self, route: &str) -> Option<&Vec<String>> {
        self.route_index.get(route)
    }

    /// Get all nodes
    pub fn all_nodes(&self) -> &HashMap<String, TransponderData> {
        &self.nodes
    }
}

impl Default for AddressBook {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl StateMachine for AddressBook {
    type Command = AddressBookCommand;
    type Response = AddressBookResponse;

    async fn apply(&mut self, command: Self::Command) -> Result<Self::Response> {
        match command {
            AddressBookCommand::Join(transponder_data) => {
                let node_id = transponder_data.node_id.clone();

                // Remove old entry if exists (upsert behavior)
                // This handles: duplicate joins, bootstrap data replacement, re-joins
                if let Some(old_data) = self.nodes.remove(&node_id) {
                    for route in &old_data.routes {
                        if let Some(nodes) = self.route_index.get_mut(route) {
                            nodes.retain(|id| id != &node_id);
                            if nodes.is_empty() {
                                self.route_index.remove(route);
                            }
                        }
                    }
                }

                // Add to route index
                for route in &transponder_data.routes {
                    self.route_index
                        .entry(route.clone())
                        .or_default()
                        .push(node_id.clone());
                }

                // Add to nodes
                self.nodes.insert(node_id, transponder_data);

                Ok(AddressBookResponse::Success)
            }

            AddressBookCommand::Leave(node_id) => {
                // Remove from nodes
                if let Some(transponder_data) = self.nodes.remove(&node_id) {
                    // Remove from route index
                    for route in &transponder_data.routes {
                        if let Some(nodes) = self.route_index.get_mut(route) {
                            nodes.retain(|id| id != &node_id);
                            // Clean up empty route entries
                            if nodes.is_empty() {
                                self.route_index.remove(route);
                            }
                        }
                    }
                    Ok(AddressBookResponse::Success)
                } else {
                    Ok(AddressBookResponse::NotFound)
                }
            }

            AddressBookCommand::Update(node_id, new_transponder_data) => {
                // Check if node exists
                if let Some(old_data) = self.nodes.get(&node_id) {
                    // Remove old routes from index
                    for route in &old_data.routes {
                        if let Some(nodes) = self.route_index.get_mut(route) {
                            nodes.retain(|id| id != &node_id);
                            if nodes.is_empty() {
                                self.route_index.remove(route);
                            }
                        }
                    }

                    // Add new routes to index
                    for route in &new_transponder_data.routes {
                        self.route_index
                            .entry(route.clone())
                            .or_default()
                            .push(node_id.clone());
                    }

                    // Update node data
                    self.nodes.insert(node_id, new_transponder_data);

                    Ok(AddressBookResponse::Success)
                } else {
                    Ok(AddressBookResponse::NotFound)
                }
            }
        }
    }

    async fn snapshot(&self) -> Result<Vec<u8>> {
        // Serialize the entire address book
        constellation_fabric::Codec::Bincode
            .encode(&(&self.nodes, &self.route_index))
            .map_err(|e| constellation_raft::Error::Serialization(e.to_string()))
    }

    async fn restore(&mut self, snapshot: Vec<u8>) -> Result<()> {
        // Deserialize and replace state
        let (nodes, route_index) = constellation_fabric::Codec::Bincode
            .decode(&snapshot)
            .map_err(|e| constellation_raft::Error::Serialization(e.to_string()))?;

        self.nodes = nodes;
        self.route_index = route_index;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{AdvertisedAddress, Capabilities};

    #[tokio::test]
    async fn test_address_book_join() {
        let mut book = AddressBook::new();

        let data = TransponderData::builder()
            .node_id("node-1")
            .transport("tcp")
            .codec("bincode")
            .route("Service.method.v1")
            .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:8080"))
            .capabilities(Capabilities::basic())
            .build();

        let response = book.apply(AddressBookCommand::Join(data)).await.unwrap();
        assert!(matches!(response, AddressBookResponse::Success));

        // Verify node was added
        assert!(book.get_node("node-1").is_some());
        assert_eq!(book.get_nodes_for_route("Service.method.v1").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_address_book_join_upsert() {
        let mut book = AddressBook::new();

        let data1 = TransponderData::builder()
            .node_id("node-1")
            .transport("tcp")
            .codec("bincode")
            .route("Service.method.v1")
            .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:8080"))
            .capabilities(Capabilities::basic())
            .build();

        book.apply(AddressBookCommand::Join(data1)).await.unwrap();

        // Join again with updated data (upsert behavior)
        let data2 = TransponderData::builder()
            .node_id("node-1")
            .transport("tcp")
            .codec("bincode")
            .route("Service.method.v2") // Different route
            .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9090")) // Different address
            .capabilities(Capabilities::basic())
            .build();

        let response = book.apply(AddressBookCommand::Join(data2)).await.unwrap();
        assert!(matches!(response, AddressBookResponse::Success));

        // Old route should be gone, new route should exist
        assert!(book.get_nodes_for_route("Service.method.v1").is_none());
        assert_eq!(book.get_nodes_for_route("Service.method.v2").unwrap().len(), 1);

        // Address should be updated
        let node = book.get_node("node-1").unwrap();
        assert_eq!(node.addresses[0].address, "127.0.0.1:9090");
    }

    #[tokio::test]
    async fn test_address_book_leave() {
        let mut book = AddressBook::new();

        let data = TransponderData::builder()
            .node_id("node-1")
            .transport("tcp")
            .codec("bincode")
            .route("Service.method.v1")
            .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:8080"))
            .capabilities(Capabilities::basic())
            .build();

        book.apply(AddressBookCommand::Join(data)).await.unwrap();

        let response = book.apply(AddressBookCommand::Leave("node-1".to_string())).await.unwrap();
        assert!(matches!(response, AddressBookResponse::Success));

        // Verify node was removed
        assert!(book.get_node("node-1").is_none());
        assert!(book.get_nodes_for_route("Service.method.v1").is_none());
    }

    #[tokio::test]
    async fn test_address_book_update() {
        let mut book = AddressBook::new();

        let data1 = TransponderData::builder()
            .node_id("node-1")
            .transport("tcp")
            .codec("bincode")
            .route("Service.method.v1")
            .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:8080"))
            .capabilities(Capabilities::basic())
            .build();

        book.apply(AddressBookCommand::Join(data1)).await.unwrap();

        let data2 = TransponderData::builder()
            .node_id("node-1")
            .transport("tcp")
            .codec("bincode")
            .route("Service.method.v2") // Different route
            .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9090"))
            .capabilities(Capabilities::basic())
            .build();

        let response = book
            .apply(AddressBookCommand::Update("node-1".to_string(), data2))
            .await
            .unwrap();

        assert!(matches!(response, AddressBookResponse::Success));

        // Old route should be gone
        assert!(book.get_nodes_for_route("Service.method.v1").is_none());

        // New route should exist
        assert_eq!(book.get_nodes_for_route("Service.method.v2").unwrap().len(), 1);
    }
}
