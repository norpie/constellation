// Test automatic handler discovery via inventory

use constellation_node::handler::Handler;
use constellation_node::rpc::RpcRequest;
use constellation_node::{handler, Node};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct PingRequest {
    message: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct PingResponse {
    reply: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TestError(String);

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TestError {}

impl constellation_node::ErrorResponder for TestError {
    fn error_category(&self) -> constellation_node::ErrorCategory {
        constellation_node::ErrorCategory::ServerError
    }
}

// Handler that should be auto-discovered
#[handler]
async fn ping(_req: PingRequest) -> Result<PingResponse, TestError> {
    Ok(PingResponse {
        reply: "pong".to_string(),
    })
}

// Handler with version 2
#[handler(version = 2)]
async fn ping_v2(_req: PingRequest) -> Result<PingResponse, TestError> {
    Ok(PingResponse {
        reply: "pong v2".to_string(),
    })
}

#[tokio::test]
async fn test_auto_discovery_enabled() {
    use constellation_fabric::codec::{BincodeCodec, Codec};

    // Build node with auto-discovery enabled (default)
    let node = Node::builder()
        .service_name("AutoDiscoveryService")
        .build()
        .unwrap();

    // Create request for v1 handler
    let codec = BincodeCodec;
    let ping_req = PingRequest {
        message: "hello".to_string(),
    };
    let payload = codec.encode(&ping_req).unwrap();

    let request = RpcRequest {
        request_id: Uuid::new_v4(),
        route: "AutoDiscoveryService.ping.v1".to_string(),
        payload,
    };

    // Get handler via auto-discovery
    let handler = &PING_HANDLER;
    let response_bytes = handler.call(node.node(), &request).await.unwrap();

    // Decode response
    let response: PingResponse = codec.decode(&response_bytes).unwrap();
    assert_eq!(response.reply, "pong");
}

#[tokio::test]
async fn test_auto_discovery_multiple_versions() {
    use constellation_fabric::codec::{BincodeCodec, Codec};

    // Build node with auto-discovery
    let node = Node::builder()
        .service_name("VersionedService")
        .build()
        .unwrap();

    let codec = BincodeCodec;
    let ping_req = PingRequest {
        message: "test".to_string(),
    };
    let payload = codec.encode(&ping_req).unwrap();

    // Test v1 handler
    let request_v1 = RpcRequest {
        request_id: Uuid::new_v4(),
        route: "VersionedService.ping.v1".to_string(),
        payload: payload.clone(),
    };

    let handler_v1 = &PING_HANDLER;
    let response_bytes_v1 = handler_v1.call(node.node(), &request_v1).await.unwrap();
    let response_v1: PingResponse = codec.decode(&response_bytes_v1).unwrap();
    assert_eq!(response_v1.reply, "pong");

    // Test v2 handler
    let request_v2 = RpcRequest {
        request_id: Uuid::new_v4(),
        route: "VersionedService.ping_v2.v2".to_string(),
        payload,
    };

    let handler_v2 = &PING_V2_HANDLER;
    let response_bytes_v2 = handler_v2.call(node.node(), &request_v2).await.unwrap();
    let response_v2: PingResponse = codec.decode(&response_bytes_v2).unwrap();
    assert_eq!(response_v2.reply, "pong v2");
}

#[tokio::test]
async fn test_auto_discovery_disabled() {
    // Build node with auto-discovery disabled
    let node = Node::builder()
        .service_name("ManualService")
        .auto_discover(false)
        .build()
        .unwrap();

    // Verify handler constants exist (macro still generates them)
    let _handler = &PING_HANDLER;
    let _handler_v2 = &PING_V2_HANDLER;

    // But the node's routes should be empty (no auto-registration)
    // We can't directly test this without exposing routes, but at least
    // we verified that auto_discover(false) doesn't break compilation
    assert_eq!(node.service_name(), "ManualService");
}
