//! Tests for the Router API
//!
//! These tests verify the router functionality for resolving routes and peers.

use constellation_node::{
    mesh::{AddressBook, AddressBookCommand, AdvertisedAddress, Capabilities, TransponderData},
    Data, Node, Router, RoutingError,
};
use constellation_raft::StateMachine;

#[tokio::test]
async fn test_router_self_reference_error() {
    // Create a node
    let node = Node::builder()
        .service_name("TestService")
        .id("node-1")
        .auto_discover(false)
        .build()
        .unwrap();

    let router: Data<Router> = node.extract().expect("Router should be auto-registered");

    // Trying to resolve self should return SelfReference error
    let result = router.peer("node-1").await;
    assert!(matches!(result, Err(RoutingError::SelfReference)));

    let result = router.resolve_peer("node-1").await;
    assert!(matches!(result, Err(RoutingError::SelfReference)));
}

#[tokio::test]
async fn test_router_peer_not_found() {
    let node = Node::builder()
        .service_name("TestService")
        .id("node-1")
        .auto_discover(false)
        .build()
        .unwrap();

    let router: Data<Router> = node.extract().unwrap();

    // Unknown peer should return PeerNotFound
    let result = router.peer("nonexistent-node").await;
    assert!(matches!(result, Err(RoutingError::PeerNotFound(_))));
}

#[tokio::test]
async fn test_router_route_not_found() {
    let node = Node::builder()
        .service_name("TestService")
        .id("node-1")
        .auto_discover(false)
        .build()
        .unwrap();

    let router: Data<Router> = node.extract().unwrap();

    // Unknown route should return RouteNotFound
    let result = router.resolve_route("NonExistent.route.v1").await;
    assert!(matches!(result, Err(RoutingError::RouteNotFound(_))));
}

#[tokio::test]
async fn test_router_any_peer_empty() {
    let node = Node::builder()
        .service_name("TestService")
        .id("node-1")
        .auto_discover(false)
        .build()
        .unwrap();

    let router: Data<Router> = node.extract().unwrap();

    // Empty address book should return None
    let result = router.any_peer().await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_router_self_id() {
    let node = Node::builder()
        .service_name("TestService")
        .id("my-node-id")
        .auto_discover(false)
        .build()
        .unwrap();

    let router: Data<Router> = node.extract().unwrap();

    assert_eq!(router.self_id(), "my-node-id");
}

#[tokio::test]
async fn test_router_auto_registered() {
    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .build()
        .unwrap();

    // Router should be auto-registered
    let router: Option<Data<Router>> = node.extract();
    assert!(router.is_some());
}

// Integration test with actual address book population
#[tokio::test]
async fn test_router_resolve_peer_with_populated_address_book() {
    // Create raft node with address book
    let mut address_book = AddressBook::new();

    // Add a peer directly (simulating what Raft apply would do)
    let peer_data = TransponderData::builder()
        .node_id("node-2")
        .transport("tcp")
        .codec("bincode")
        .route("TestService.method.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9002"))
        .capabilities(Capabilities::basic())
        .build();

    address_book
        .apply(AddressBookCommand::Join(peer_data))
        .await
        .unwrap();

    // Create raft with populated address book
    let raft = constellation_raft::RaftNode::builder()
        .node_id("node-1".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("node-1".to_string(), raft);

    // Now resolve_peer should work
    let result = router.resolve_peer("node-2").await;
    assert!(result.is_ok());

    let target = result.unwrap();
    assert_eq!(target.peer_id, "node-2");
    assert_eq!(target.transport, "tcp");
    assert_eq!(target.address, "127.0.0.1:9002");
}

#[tokio::test]
async fn test_router_resolve_route_with_populated_address_book() {
    let mut address_book = AddressBook::new();

    // Add a peer that handles a route
    let peer_data = TransponderData::builder()
        .node_id("node-2")
        .transport("tcp")
        .codec("bincode")
        .route("TestService.method.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9003"))
        .capabilities(Capabilities::basic())
        .build();

    address_book
        .apply(AddressBookCommand::Join(peer_data))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("node-1".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("node-1".to_string(), raft);

    // Resolve route should work
    let result = router.resolve_route("TestService.method.v1").await;
    assert!(result.is_ok());

    let target = result.unwrap();
    assert_eq!(target.peer_id, "node-2");
    assert_eq!(target.transport, "tcp");
    assert_eq!(target.address, "127.0.0.1:9003");
}

#[tokio::test]
async fn test_router_any_peer_with_populated_address_book() {
    let mut address_book = AddressBook::new();

    // Add multiple peers
    let peer2_data = TransponderData::builder()
        .node_id("node-2")
        .transport("tcp")
        .codec("bincode")
        .route("SomeRoute.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9004"))
        .capabilities(Capabilities::basic())
        .build();

    let peer3_data = TransponderData::builder()
        .node_id("node-3")
        .transport("tcp")
        .codec("bincode")
        .route("AnotherRoute.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9005"))
        .capabilities(Capabilities::basic())
        .build();

    address_book
        .apply(AddressBookCommand::Join(peer2_data))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(peer3_data))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("node-1".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("node-1".to_string(), raft);

    // any_peer should return one of the peers (not self)
    let result = router.any_peer().await;
    assert!(result.is_some());
    let peer_id = result.unwrap();
    assert!(peer_id == "node-2" || peer_id == "node-3");
    assert_ne!(peer_id, "node-1"); // Should not be self
}

#[tokio::test]
async fn test_router_round_robin() {
    let mut address_book = AddressBook::new();

    // Add multiple peers that handle the same route
    let peer2_data = TransponderData::builder()
        .node_id("node-2")
        .transport("tcp")
        .codec("bincode")
        .route("SharedRoute.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9006"))
        .capabilities(Capabilities::basic())
        .build();

    let peer3_data = TransponderData::builder()
        .node_id("node-3")
        .transport("tcp")
        .codec("bincode")
        .route("SharedRoute.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9007"))
        .capabilities(Capabilities::basic())
        .build();

    address_book
        .apply(AddressBookCommand::Join(peer2_data))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(peer3_data))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("node-1".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("node-1".to_string(), raft);

    // Make multiple calls and verify we get different peers
    let mut seen_node2 = false;
    let mut seen_node3 = false;

    for _ in 0..10 {
        let target = router.resolve_route("SharedRoute.v1").await.unwrap();
        if target.peer_id == "node-2" {
            seen_node2 = true;
        }
        if target.peer_id == "node-3" {
            seen_node3 = true;
        }
    }

    // Should have seen both peers due to round-robin
    assert!(seen_node2, "Should have routed to node-2");
    assert!(seen_node3, "Should have routed to node-3");
}

#[tokio::test]
async fn test_router_skips_self_in_route_resolution() {
    let mut address_book = AddressBook::new();

    // Add self and another peer handling the same route
    let self_data = TransponderData::builder()
        .node_id("node-1") // Same as router's self_id
        .transport("tcp")
        .codec("bincode")
        .route("SharedRoute.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9008"))
        .capabilities(Capabilities::basic())
        .build();

    let peer_data = TransponderData::builder()
        .node_id("node-2")
        .transport("tcp")
        .codec("bincode")
        .route("SharedRoute.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9009"))
        .capabilities(Capabilities::basic())
        .build();

    address_book
        .apply(AddressBookCommand::Join(self_data))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(peer_data))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("node-1".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("node-1".to_string(), raft);

    // Route resolution should always skip self
    for _ in 0..5 {
        let target = router.resolve_route("SharedRoute.v1").await.unwrap();
        assert_eq!(target.peer_id, "node-2", "Should never route to self");
    }
}

#[tokio::test]
async fn test_router_route_only_self_returns_error() {
    let mut address_book = AddressBook::new();

    // Add only self handling a route
    let self_data = TransponderData::builder()
        .node_id("node-1") // Same as router's self_id
        .transport("tcp")
        .codec("bincode")
        .route("OnlySelfRoute.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9010"))
        .capabilities(Capabilities::basic())
        .build();

    address_book
        .apply(AddressBookCommand::Join(self_data))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("node-1".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("node-1".to_string(), raft);

    // Should return RouteNotFound since only self handles it
    let result = router.resolve_route("OnlySelfRoute.v1").await;
    assert!(matches!(result, Err(RoutingError::RouteNotFound(_))));
}
