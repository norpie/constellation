// Integration tests for node runtime

use constellation_fabric::Codec;
use constellation_fabric::transport::{TcpTransport, TcpTransportListener, Transport};
use constellation_node::mesh::{AddressBook, AddressBookCommand, AdvertisedAddress, Capabilities, TransponderData};
use constellation_node::{handler, Config, Data, Node, RpcClient};
use constellation_raft::StateMachine;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::RwLock;
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
        .listen(listener, "test", "tcp", addr.to_string())
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
    let codec = Codec::Bincode;
    let add_req = AddRequest { a: 5, b: 3 };
    let payload = codec.encode(&add_req).unwrap();

    // Pack RPC frame
    let header = constellation_node::rpc::RpcHeader {
        request_id: Uuid::new_v4(),
        route: "MathService.add.v1".to_string(),
        trace_id: None,
        parent_span_id: None,
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
        .listen(listener, "test", "tcp", addr.to_string())
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
    let codec = Codec::Bincode;
    let add_req = AddRequest { a: 5, b: 3 };
    let payload = codec.encode(&add_req).unwrap();

    // Pack RPC frame
    let header = constellation_node::rpc::RpcHeader {
        request_id: Uuid::new_v4(),
        route: "MathService.failing.v1".to_string(),
        trace_id: None,
        parent_span_id: None,
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

/// Test RPC calls using RpcClient (full end-to-end with routing)
#[tokio::test]
async fn test_rpc_client_end_to_end() {
    // Setup server listener
    let listener = TcpTransportListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    // Build and start server node
    let server_node = Node::builder()
        .service_name("MathService")
        .id("server-node")
        .auto_discover(false)
        .register("add.v1", &ADD_HANDLER)
        .listen(listener, "test", "tcp", addr.to_string())
        .build()
        .unwrap();

    // Spawn server in background
    tokio::spawn(async move {
        let _ = server_node.start().await;
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Build client node with server in address book
    let mut address_book = AddressBook::new();

    // Add server to address book
    let server_data = TransponderData::builder()
        .node_id("server-node")
        .transport("tcp")
        .codec("bincode")
        .route("MathService.add.v1")
        .address(AdvertisedAddress::new("default", "tcp", &addr.to_string()))
        .capabilities(Capabilities::basic())
        .build();

    address_book
        .apply(AddressBookCommand::Join(server_data))
        .await
        .unwrap();

    // Create client's raft with populated address book
    let client_raft = constellation_raft::RaftNode::builder()
        .node_id("client-node".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    // Create router and rpc client manually for this test
    let router = constellation_node::Router::new("client-node".to_string(), client_raft);
    let config = Data::new(RwLock::new(Config::default()));
    let rpc = RpcClient::new(router, config);

    // Make RPC call using RpcClient
    let request = AddRequest { a: 10, b: 20 };
    let response: AddResponse = rpc
        .call("MathService.add.v1", &request)
        .expect("Serialization should succeed")
        .await
        .expect("RPC call should succeed");

    // Verify response
    assert_eq!(response.result, 30);
}

/// Test RPC call to specific peer using call_peer
#[tokio::test]
async fn test_rpc_client_call_peer() {
    // Setup server listener
    let listener = TcpTransportListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    // Build and start server node
    let server_node = Node::builder()
        .service_name("MathService")
        .id("server-node")
        .auto_discover(false)
        .register("add.v1", &ADD_HANDLER)
        .listen(listener, "test", "tcp", addr.to_string())
        .build()
        .unwrap();

    // Spawn server in background
    tokio::spawn(async move {
        let _ = server_node.start().await;
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Build client with server in address book
    let mut address_book = AddressBook::new();

    let server_data = TransponderData::builder()
        .node_id("server-node")
        .transport("tcp")
        .codec("bincode")
        .route("MathService.add.v1")
        .address(AdvertisedAddress::new("default", "tcp", &addr.to_string()))
        .capabilities(Capabilities::basic())
        .build();

    address_book
        .apply(AddressBookCommand::Join(server_data))
        .await
        .unwrap();

    let client_raft = constellation_raft::RaftNode::builder()
        .node_id("client-node".to_string())
        .storage(constellation_raft::MemoryStorage::new())
        .state_machine(address_book)
        .build()
        .unwrap();

    let router = constellation_node::Router::new("client-node".to_string(), client_raft);
    let config = Data::new(RwLock::new(Config::default()));
    let rpc = RpcClient::new(router, config);

    // Make RPC call directly to peer
    let request = AddRequest { a: 7, b: 8 };
    let response: AddResponse = rpc
        .call_peer("server-node", "MathService.add.v1", &request)
        .expect("Serialization should succeed")
        .await
        .expect("RPC call should succeed");

    // Verify response
    assert_eq!(response.result, 15);
}

#[tokio::test]
async fn test_telemetry_context_flows_through_handlers() {
    use constellation_telemetry::{BufferCollector, Collector, TelemetryEntry};

    // Setup listener
    let listener = TcpTransportListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    // Build node with telemetry enabled (default)
    let node = Node::builder()
        .service_name("TelemetryTest")
        .id("telemetry-node")
        .auto_discover(false)
        .register("add.v1", &ADD_HANDLER)
        .binding(
            constellation_node::Binding::new(listener, "tcp")
                .advertise("default", &addr.to_string()),
        )
        .build()
        .unwrap();

    // Extract collector before starting (to verify it exists)
    let collector: Data<BufferCollector> = node
        .extract()
        .expect("BufferCollector should be registered when telemetry is enabled");

    // Spawn node runtime in background
    tokio::spawn(async move {
        let _ = node.start().await;
    });

    // Give node time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Build and send request with trace context
    let trace_id = constellation_telemetry::TraceId::new();
    let span_id = constellation_telemetry::SpanId::new();

    let header = constellation_node::rpc::RpcHeader {
        request_id: Uuid::new_v4(),
        route: "TelemetryTest.add.v1".to_string(),
        trace_id: Some(trace_id.clone()),
        parent_span_id: Some(span_id.clone()),
    };

    let payload = Codec::Bincode
        .encode(&AddRequest { a: 10, b: 20 })
        .unwrap();
    let frame = constellation_node::rpc::pack_frame(&header, &payload).unwrap();

    // Send request
    let mut client = TcpTransport::connect(addr).await.unwrap();
    client.send(&frame).await.unwrap();

    // Receive response
    let response_frame = client.receive().await.unwrap();
    let (_resp_header, resp_payload) =
        constellation_node::rpc::parse_frame(&response_frame).unwrap();
    let response: constellation_node::rpc::RpcResponse =
        Codec::Bincode.decode(resp_payload).unwrap();

    // Verify response succeeded
    match response.result {
        constellation_node::rpc::ResponseResult::Success(payload) => {
            let add_response: AddResponse = Codec::Bincode.decode(&payload).unwrap();
            assert_eq!(add_response.result, 30);
        }
        _ => panic!("Expected success response"),
    }

    // Drain collector and verify spans were created
    let entries = collector.drain();

    // We should have at least 3 spans: rpc.server/{route}, deserialize, serialize
    let spans: Vec<_> = entries
        .iter()
        .filter_map(|e| match e {
            TelemetryEntry::Span(s) => Some(s),
            _ => None,
        })
        .collect();

    assert!(
        spans.len() >= 3,
        "Expected at least 3 spans, got {}",
        spans.len()
    );

    // Verify we have the expected span names
    let span_names: Vec<_> = spans.iter().map(|s| s.name.as_str()).collect();
    assert!(
        span_names.iter().any(|n| n.starts_with("rpc.server/")),
        "Expected rpc.server span, got {:?}",
        span_names
    );
    assert!(
        span_names.contains(&"deserialize"),
        "Expected deserialize span, got {:?}",
        span_names
    );
    assert!(
        span_names.contains(&"serialize"),
        "Expected serialize span, got {:?}",
        span_names
    );

    // Verify trace_id was propagated for handler spans (not framework task spans)
    // Framework tasks (like leader_heartbeat) get fresh traces, which is correct
    let handler_spans: Vec<_> = spans
        .iter()
        .filter(|s| {
            s.name.starts_with("rpc.server/")
                || s.name == "deserialize"
                || s.name == "serialize"
        })
        .collect();

    assert!(
        handler_spans.len() >= 3,
        "Expected at least 3 handler spans, got {}",
        handler_spans.len()
    );

    for span in handler_spans {
        assert_eq!(
            span.common.trace_id.as_ref().map(|t| t.as_str()),
            Some(trace_id.as_str()),
            "Handler span {} should have propagated trace_id",
            span.name
        );
    }
}
