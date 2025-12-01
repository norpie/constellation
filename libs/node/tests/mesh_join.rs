// Test built-in mesh join handler

use constellation_fabric::codec::{BincodeCodec, Codec};
use constellation_node::mesh::{AddressGroup, Capabilities, JoinResponse, TransponderData};
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
async fn test_join_response_serialization() {
    let codec = BincodeCodec;

    // Test Success variant
    let success = JoinResponse::Success;
    let bytes = codec.encode(&success).unwrap();
    let decoded: JoinResponse = codec.decode(&bytes).unwrap();
    assert!(matches!(decoded, JoinResponse::Success));

    // Test NotLeader with None
    let not_leader = JoinResponse::NotLeader { leader: None };
    let bytes = codec.encode(&not_leader).unwrap();
    let decoded: JoinResponse = codec.decode(&bytes).unwrap();
    assert!(matches!(decoded, JoinResponse::NotLeader { leader: None }));

    // Test NotLeader with Some leader data
    let leader_data = TransponderData::builder()
        .node_id("leader-node")
        .transport("tcp")
        .codec("bincode")
        .address(AddressGroup::single("default", "tcp", "127.0.0.1:8080"))
        .capabilities(Capabilities::basic())
        .build();

    let not_leader_with_data = JoinResponse::NotLeader {
        leader: Some(leader_data),
    };
    let bytes = codec.encode(&not_leader_with_data).unwrap();
    let decoded: JoinResponse = codec.decode(&bytes).unwrap();

    match decoded {
        JoinResponse::NotLeader { leader: Some(data) } => {
            assert_eq!(data.node_id, "leader-node");
        }
        _ => panic!("Expected NotLeader with leader data"),
    }
}
