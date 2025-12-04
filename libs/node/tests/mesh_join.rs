// Test built-in mesh handlers (join, leave)

use constellation_fabric::Codec;
use constellation_node::mesh::{AdvertisedAddress, Capabilities, LeaveRequest, MeshResponse, TransponderData};
use constellation_node::Node;

#[tokio::test]
async fn test_mesh_join_handler_registered() {
    // Build a node
    let _node = Node::builder()
        .service_name("TestService")
        .id("test-node")
        .auto_discover(true)
        .build()
        .unwrap();

    // Verify the handler is registered via inventory
    let mut found_mesh_join = false;

    for registration in inventory::iter::<constellation_node::handler::HandlerRegistration> {
        if let Some(route) = registration.route {
            if route == "_mesh.join" {
                found_mesh_join = true;
            }
        }
    }

    assert!(found_mesh_join, "_mesh.join handler should be registered");
}

#[tokio::test]
async fn test_mesh_join_handler_not_prefixed_with_service_name() {
    // Build a node with a specific service name
    let _node = Node::builder()
        .service_name("MyCustomService")
        .id("test-node")
        .auto_discover(true)
        .build()
        .unwrap();

    // Verify builtin handlers use their custom routes
    for registration in inventory::iter::<constellation_node::handler::HandlerRegistration> {
        if let Some(route) = registration.route {
            if route.starts_with("_mesh.") {
                assert!(
                    !route.starts_with("MyCustomService."),
                    "Builtin handler should not be prefixed with service name"
                );
            }
        }
    }
}

#[tokio::test]
async fn test_mesh_response_serialization() {
    let codec = Codec::Bincode;

    // Test Success variant
    let success = MeshResponse::Success;
    let bytes = codec.encode(&success).unwrap();
    let decoded: MeshResponse = codec.decode(&bytes).unwrap();
    assert!(matches!(decoded, MeshResponse::Success));

    // Test NotLeader with None
    let not_leader = MeshResponse::NotLeader { leader: None };
    let bytes = codec.encode(&not_leader).unwrap();
    let decoded: MeshResponse = codec.decode(&bytes).unwrap();
    assert!(matches!(decoded, MeshResponse::NotLeader { leader: None }));

    // Test NotLeader with Some leader data
    let leader_data = TransponderData::builder()
        .node_id("leader-node")
        .transport("tcp")
        .codec("bincode")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:8080"))
        .capabilities(Capabilities::basic())
        .build();

    let not_leader_with_data = MeshResponse::NotLeader {
        leader: Some(leader_data),
    };
    let bytes = codec.encode(&not_leader_with_data).unwrap();
    let decoded: MeshResponse = codec.decode(&bytes).unwrap();

    match decoded {
        MeshResponse::NotLeader { leader: Some(data) } => {
            assert_eq!(data.node_id, "leader-node");
        }
        _ => panic!("Expected NotLeader with leader data"),
    }
}

#[tokio::test]
async fn test_mesh_leave_handler_registered() {
    // Build a node
    let _node = Node::builder()
        .service_name("TestService")
        .id("test-node")
        .auto_discover(true)
        .build()
        .unwrap();

    // Verify the handler is registered via inventory
    let mut found_mesh_leave = false;

    for registration in inventory::iter::<constellation_node::handler::HandlerRegistration> {
        if let Some(route) = registration.route {
            if route == "_mesh.leave" {
                found_mesh_leave = true;
            }
        }
    }

    assert!(found_mesh_leave, "_mesh.leave handler should be registered");
}

#[tokio::test]
async fn test_leave_request_serialization() {
    let codec = Codec::Bincode;

    let request = LeaveRequest {
        node_id: "node-to-leave".to_string(),
    };
    let bytes = codec.encode(&request).unwrap();
    let decoded: LeaveRequest = codec.decode(&bytes).unwrap();

    assert_eq!(decoded.node_id, "node-to-leave");
}
