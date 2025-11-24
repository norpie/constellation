// Integration tests for node runtime

use constellation_fabric::codec::{BincodeCodec, Codec};
use constellation_fabric::transport::{TcpTransport, TcpTransportListener, Transport};
use constellation_node::{handler, Node};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct AddRequest {
    a: i32,
    b: i32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct AddResponse {
    result: i32,
}

#[derive(Debug)]
struct TestError(String);

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TestError {}

#[handler]
async fn add(req: AddRequest) -> Result<AddResponse, TestError> {
    Ok(AddResponse { result: req.a + req.b })
}

#[tokio::test]
async fn test_basic_rpc_flow() {
    // Setup listener
    let listener = TcpTransportListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    // Build and start node
    let node = Node::builder()
        .service_name("MathService")
        .auto_discover(false)
        .register("add.v1", &ADD_HANDLER)
        .listen(listener, "test")
        .build()
        .unwrap();

    // Spawn node runtime in background
    tokio::spawn(async move {
        // Don't wait for ctrl_c in test - just run forever
        let _ = node.start().await;
    });

    // Give node time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect as client
    let mut client = TcpTransport::connect(addr).await.unwrap();

    // Build request
    let codec = BincodeCodec;
    let add_req = AddRequest { a: 5, b: 3 };
    let payload = codec.encode(&add_req).unwrap();

    // Pack RPC frame
    let header = constellation_node::rpc::RpcHeader {
        request_id: Uuid::new_v4(),
        route: "MathService.add.v1".to_string(),
    };
    let frame = constellation_node::rpc::pack_frame(&header, &payload).unwrap();

    // Send request
    client.send(&frame).await.unwrap();

    // Receive response
    let response_frame = client.receive().await.unwrap();

    // Parse response frame
    let (response_header, response_payload) =
        constellation_node::rpc::parse_frame(&response_frame).unwrap();

    // Verify header
    assert_eq!(response_header.request_id, header.request_id);
    assert_eq!(response_header.route, header.route);

    // Decode response
    let response: AddResponse = codec.decode(response_payload).unwrap();
    assert_eq!(response.result, 8);
}
