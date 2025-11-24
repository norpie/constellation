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

#[handler]
async fn add(req: AddRequest) -> Result<AddResponse, TestError> {
    Ok(AddResponse { result: req.a + req.b })
}

#[handler]
async fn failing_handler(_req: AddRequest) -> Result<AddResponse, TestError> {
    Err(TestError("Intentional test failure".to_string()))
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

    // Decode RpcResponse
    use constellation_node::rpc::{ResponseResult, RpcResponse};
    let rpc_response: RpcResponse = codec.decode(response_payload).unwrap();
    assert_eq!(rpc_response.request_id, header.request_id);

    // Extract success payload
    match rpc_response.result {
        ResponseResult::Success(payload) => {
            let response: AddResponse = codec.decode(&payload).unwrap();
            assert_eq!(response.result, 8);
        }
        ResponseResult::Error { category, payload: _ } => {
            panic!("Expected success, got error: {:?}", category);
        }
    }
}

#[tokio::test]
async fn test_error_response() {
    // Setup listener
    let listener = TcpTransportListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    // Build and start node with failing handler
    let node = Node::builder()
        .service_name("MathService")
        .auto_discover(false)
        .register("failing.v1", &FAILING_HANDLER_HANDLER)
        .listen(listener, "test")
        .build()
        .unwrap();

    // Spawn node runtime in background
    tokio::spawn(async move {
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
        route: "MathService.failing.v1".to_string(),
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

    // Decode RpcResponse
    use constellation_node::rpc::{ErrorCategory, ResponseResult, RpcResponse};
    let rpc_response: RpcResponse = codec.decode(response_payload).unwrap();
    assert_eq!(rpc_response.request_id, header.request_id);

    // Extract error payload
    match rpc_response.result {
        ResponseResult::Success(_) => {
            panic!("Expected error, got success");
        }
        ResponseResult::Error { category, payload } => {
            // Verify category
            assert!(matches!(category, ErrorCategory::ServerError));

            // Decode error
            let error: TestError = codec.decode(&payload).unwrap();
            assert_eq!(error.0, "Intentional test failure");
        }
    }
}
