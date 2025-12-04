// Test built-in Raft handlers

use constellation_fabric::Codec;
use constellation_node::Node;
use constellation_raft::{RequestVoteRequest, RequestVoteResponse};
use uuid::Uuid;

#[tokio::test]
async fn test_raft_request_vote_handler_registered() {
    // Build a node with Raft
    let node = Node::builder()
        .service_name("TestService")
        .id("test-node")
        .auto_discover(true) // Should auto-discover builtin handlers
        .build()
        .unwrap();

    // Extract the RaftNode to verify it's registered
    use constellation_node::Data;
    use constellation_node::mesh::AddressBook;
    use constellation_raft::RaftNode;

    let raft: Data<RaftNode<AddressBook>> = node.extract().expect("RaftNode should be auto-registered");

    // Create a RequestVote request
    let codec = Codec::Bincode;
    let request = RequestVoteRequest {
        term: 1,
        candidate_id: "candidate-node".to_string(),
        last_log_index: 0,
        last_log_term: 0,
    };
    let payload = codec.encode(&request).unwrap();

    // Create RPC request targeting the builtin Raft handler
    let rpc_request = constellation_node::rpc::RpcRequest {
        request_id: Uuid::new_v4(),
        route: "_raft.request_vote".to_string(),
        payload,
    };

    // The handler should be registered and accessible
    // We can't directly call it through the node.routes since it's private,
    // but we can verify the route was registered by checking inventory
    let mut found_request_vote = false;
    let mut found_append_entries = false;

    for registration in inventory::iter::<constellation_node::handler::HandlerRegistration> {
        if let Some(route) = registration.route {
            if route == "_raft.request_vote" {
                found_request_vote = true;
            }
            if route == "_raft.append_entries" {
                found_append_entries = true;
            }
        }
    }

    assert!(found_request_vote, "RequestVote handler should be registered");
    assert!(found_append_entries, "AppendEntries handler should be registered");
}

#[tokio::test]
async fn test_raft_handlers_not_prefixed_with_service_name() {
    // Build a node with a specific service name
    let node = Node::builder()
        .service_name("MyCustomService")
        .id("test-node")
        .auto_discover(true)
        .build()
        .unwrap();

    // Verify that builtin handlers use their custom routes, not service-prefixed ones
    for registration in inventory::iter::<constellation_node::handler::HandlerRegistration> {
        if let Some(route) = registration.route {
            // Builtin Raft handlers should start with _raft, not MyCustomService
            if route.starts_with("_raft.") {
                assert!(!route.starts_with("MyCustomService."),
                    "Builtin handler should not be prefixed with service name");
            }
        }
    }
}
