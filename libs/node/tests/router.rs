//! Tests for the Router API
//!
//! These tests verify the router functionality for resolving routes and peers,
//! including locality ranking and constraint filtering.

use constellation_fabric::Codec;
use constellation_node::{
    mesh::{
        AddressBook, AddressBookCommand, AdvertisedAddress, Capabilities, ConnectionRules,
        Constraint, TransponderData,
    },
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

// ============================================================================
// Locality Ranking Tests
// ============================================================================

#[tokio::test]
async fn test_router_locality_prefers_same_zone() {
    let mut address_book = AddressBook::new();

    // Caller is in us-east-1a
    let caller_data = TransponderData::builder()
        .node_id("caller")
        .region("us-east")
        .zone("us-east-1a")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.1:8080"))
        .build();

    // Node in same zone (should be preferred)
    let same_zone = TransponderData::builder()
        .node_id("same-zone")
        .region("us-east")
        .zone("us-east-1a")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.2:8080"))
        .build();

    // Node in different region
    let different_region = TransponderData::builder()
        .node_id("different-region")
        .region("eu-west")
        .zone("eu-west-1a")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.2.1:8080"))
        .build();

    address_book
        .apply(AddressBookCommand::Join(caller_data))
        .await
        .unwrap();
    // Add different-region first to ensure locality wins over insertion order
    address_book
        .apply(AddressBookCommand::Join(different_region))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(same_zone))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("caller".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("caller".to_string(), raft);

    // Should always prefer same-zone node
    let target = router.resolve_route("TestRoute.v1").await.unwrap();
    assert_eq!(target.peer_id, "same-zone", "Should prefer same zone");
}

#[tokio::test]
async fn test_router_locality_prefers_same_region_over_other() {
    let mut address_book = AddressBook::new();

    // Caller is in us-east-1a
    let caller_data = TransponderData::builder()
        .node_id("caller")
        .region("us-east")
        .zone("us-east-1a")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.1:8080"))
        .build();

    // Node in same region, different zone (should be preferred over different region)
    let same_region = TransponderData::builder()
        .node_id("same-region")
        .region("us-east")
        .zone("us-east-1b") // Different zone but same region
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.3:8080"))
        .build();

    // Node in different region
    let different_region = TransponderData::builder()
        .node_id("different-region")
        .region("eu-west")
        .zone("eu-west-1a")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.2.1:8080"))
        .build();

    address_book
        .apply(AddressBookCommand::Join(caller_data))
        .await
        .unwrap();
    // Add different-region first
    address_book
        .apply(AddressBookCommand::Join(different_region))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(same_region))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("caller".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("caller".to_string(), raft);

    // Should prefer same-region node over different-region
    let target = router.resolve_route("TestRoute.v1").await.unwrap();
    assert_eq!(
        target.peer_id, "same-region",
        "Should prefer same region over different region"
    );
}

#[tokio::test]
async fn test_router_locality_global_nodes_match() {
    let mut address_book = AddressBook::new();

    // Caller uses default "global" region/zone
    let caller_data = TransponderData::builder()
        .node_id("caller")
        // No region/zone set - defaults to "global"
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.1:8080"))
        .build();

    // Node also using default "global"
    let global_node = TransponderData::builder()
        .node_id("global-node")
        // No region/zone set - defaults to "global"
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.2:8080"))
        .build();

    // Node with explicit region
    let regional_node = TransponderData::builder()
        .node_id("regional-node")
        .region("us-east")
        .zone("us-east-1a")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.2.1:8080"))
        .build();

    address_book
        .apply(AddressBookCommand::Join(caller_data))
        .await
        .unwrap();
    // Add regional first
    address_book
        .apply(AddressBookCommand::Join(regional_node))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(global_node))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("caller".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("caller".to_string(), raft);

    // Two "global" nodes should be considered same zone
    let target = router.resolve_route("TestRoute.v1").await.unwrap();
    assert_eq!(
        target.peer_id, "global-node",
        "Two global nodes should match as same zone"
    );
}

// ============================================================================
// Constraint Filtering Tests
// ============================================================================

#[tokio::test]
async fn test_router_constraint_blocks_transport() {
    let mut address_book = AddressBook::new();

    // Caller only allows TLS transport
    let caller_data = TransponderData::builder()
        .node_id("caller")
        .transport("tls")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tls", "10.0.1.1:8080"))
        .global_constraints(
            Constraint::allow_all().with_default(ConnectionRules::only_transport("tls")),
        )
        .build();

    // Target only has TCP address (not TLS)
    let target_data = TransponderData::builder()
        .node_id("target")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.2:8080"))
        .build();

    address_book
        .apply(AddressBookCommand::Join(caller_data))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(target_data))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("caller".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("caller".to_string(), raft);

    // Should fail - caller requires TLS but target only has TCP
    let result = router.resolve_peer("target").await;
    assert!(
        matches!(result, Err(RoutingError::NoAddressAvailable(_))),
        "Should fail when transport doesn't match constraints"
    );
}

#[tokio::test]
async fn test_router_constraint_blocks_codec() {
    let mut address_book = AddressBook::new();

    // Caller only allows JSON codec
    let caller_data = TransponderData::builder()
        .node_id("caller")
        .transport("tcp")
        .codec("json")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.1:8080"))
        .global_constraints(
            Constraint::allow_all().with_default(ConnectionRules::only_codec(Codec::Json)),
        )
        .build();

    // Target address only supports Bincode (not JSON)
    let mut target_addr = AdvertisedAddress::new("internal", "tcp", "10.0.1.2:8080");
    target_addr.codecs = vec![Codec::Bincode]; // Only bincode, no JSON

    let target_data = TransponderData::builder()
        .node_id("target")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(target_addr)
        .build();

    address_book
        .apply(AddressBookCommand::Join(caller_data))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(target_data))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("caller".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("caller".to_string(), raft);

    // Should fail - no codec intersection
    let result = router.resolve_peer("target").await;
    assert!(
        matches!(result, Err(RoutingError::NoAddressAvailable(_))),
        "Should fail when codec doesn't match constraints"
    );
}

#[tokio::test]
async fn test_router_constraint_per_network() {
    let mut address_book = AddressBook::new();

    // Caller requires TLS on external network, allows anything on internal
    let caller_data = TransponderData::builder()
        .node_id("caller")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.1:8080"))
        .global_constraints(
            Constraint::allow_all()
                .with_network("external", ConnectionRules::only_transport("tls")),
        )
        .build();

    // Target has internal TCP address (should work)
    let target_data = TransponderData::builder()
        .node_id("target")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.2:8080"))
        .build();

    address_book
        .apply(AddressBookCommand::Join(caller_data))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(target_data))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("caller".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("caller".to_string(), raft);

    // Should succeed - internal network allows TCP
    let result = router.resolve_peer("target").await;
    assert!(result.is_ok(), "Internal TCP should be allowed");
}

#[tokio::test]
async fn test_router_codec_intersection() {
    let mut address_book = AddressBook::new();

    // Caller allows Bincode and JSON
    let caller_data = TransponderData::builder()
        .node_id("caller")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.1:8080"))
        .global_constraints(
            Constraint::allow_all()
                .with_default(ConnectionRules::only_codecs([Codec::Bincode, Codec::Json])),
        )
        .build();

    // Target address supports Bincode and MessagePack
    let mut target_addr = AdvertisedAddress::new("internal", "tcp", "10.0.1.2:8080");
    target_addr.codecs = vec![Codec::Bincode, Codec::MessagePack];

    let target_data = TransponderData::builder()
        .node_id("target")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(target_addr)
        .build();

    address_book
        .apply(AddressBookCommand::Join(caller_data))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(target_data))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("caller".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("caller".to_string(), raft);

    // Should succeed with Bincode as the intersection
    let result = router.resolve_peer("target").await;
    assert!(result.is_ok(), "Should find codec intersection");

    let target = result.unwrap();
    assert_eq!(target.codecs, vec![Codec::Bincode], "Intersection is Bincode");
}

#[tokio::test]
async fn test_router_falls_back_to_next_node_on_constraint_failure() {
    let mut address_book = AddressBook::new();

    // Caller in us-east-1a, requires TLS
    let caller_data = TransponderData::builder()
        .node_id("caller")
        .region("us-east")
        .zone("us-east-1a")
        .transport("tls")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tls", "10.0.1.1:8080"))
        .global_constraints(
            Constraint::allow_all().with_default(ConnectionRules::only_transport("tls")),
        )
        .build();

    // Closest node (same zone) but only has TCP - will be filtered
    let close_tcp_only = TransponderData::builder()
        .node_id("close-tcp")
        .region("us-east")
        .zone("us-east-1a")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.2:8080"))
        .build();

    // Further node (different region) but has TLS - should be selected
    let far_tls = TransponderData::builder()
        .node_id("far-tls")
        .region("eu-west")
        .zone("eu-west-1a")
        .transport("tls")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tls", "10.0.2.1:8080"))
        .build();

    address_book
        .apply(AddressBookCommand::Join(caller_data))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(close_tcp_only))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(far_tls))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("caller".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("caller".to_string(), raft);

    // Should fall back to far-tls since close-tcp doesn't match constraints
    let result = router.resolve_route("TestRoute.v1").await;
    assert!(result.is_ok(), "Should fall back to compatible node");

    let target = result.unwrap();
    assert_eq!(
        target.peer_id, "far-tls",
        "Should select further node that matches constraints"
    );
}

#[tokio::test]
async fn test_router_bootstrap_uses_defaults() {
    let mut address_book = AddressBook::new();

    // Only target in address book (caller not registered yet - bootstrap scenario)
    let target_data = TransponderData::builder()
        .node_id("target")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.2:8080"))
        .build();

    address_book
        .apply(AddressBookCommand::Join(target_data))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("caller".to_string()) // Caller not in address book
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("caller".to_string(), raft);

    // Should succeed using default constraints (allow all)
    let result = router.resolve_peer("target").await;
    assert!(
        result.is_ok(),
        "Bootstrap should work with default constraints"
    );
}

#[tokio::test]
async fn test_router_resolved_target_has_codecs() {
    let mut address_book = AddressBook::new();

    let caller_data = TransponderData::builder()
        .node_id("caller")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(AdvertisedAddress::new("internal", "tcp", "10.0.1.1:8080"))
        .build();

    // Target with multiple codecs
    let mut target_addr = AdvertisedAddress::new("internal", "tcp", "10.0.1.2:8080");
    target_addr.codecs = vec![Codec::Bincode, Codec::Json, Codec::MessagePack];

    let target_data = TransponderData::builder()
        .node_id("target")
        .transport("tcp")
        .codec("bincode")
        .route("TestRoute.v1")
        .address(target_addr)
        .build();

    address_book
        .apply(AddressBookCommand::Join(caller_data))
        .await
        .unwrap();
    address_book
        .apply(AddressBookCommand::Join(target_data))
        .await
        .unwrap();

    let raft = constellation_raft::RaftNode::builder()
        .node_id("caller".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = Router::new("caller".to_string(), raft);

    let target = router.resolve_peer("target").await.unwrap();

    // Should have all codecs (caller has no restrictions)
    assert!(!target.codecs.is_empty(), "Should have codecs");
    assert!(
        target.codecs.contains(&Codec::Bincode),
        "Should include Bincode"
    );
}
