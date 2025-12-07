// Integration tests for node runtime

use constellation_fabric::channel::Channel;
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

    // Connect as client using Channel
    let mut channel = Channel::tcp(addr, Codec::Bincode).await.unwrap();

    // Build request
    let add_req = AddRequest { a: 5, b: 3 };
    let payload = channel.codec().encode(&add_req).unwrap();

    // Build RPC header
    let header = constellation_node::rpc::RpcHeader {
        request_id: Uuid::new_v4(),
        route: "MathService.add.v1".to_string(),
        trace_id: None,
        parent_span_id: None,
    };

    // Send framed request
    channel.send_framed(&header, &payload).await.unwrap();

    // Receive framed response
    let (response_header, response_payload): (constellation_node::rpc::RpcHeader, Vec<u8>) =
        channel.receive_framed().await.unwrap();

    // Verify header
    assert_eq!(response_header.request_id, header.request_id);
    assert_eq!(response_header.route, header.route);

    // Decode RpcResponse
    use constellation_node::rpc::{ResponseResult, RpcResponse};
    let rpc_response: RpcResponse = channel.codec().decode(&response_payload).unwrap();
    assert_eq!(rpc_response.request_id, header.request_id);

    // Extract success payload
    match rpc_response.result {
        ResponseResult::Success(payload) => {
            let response: AddResponse = channel.codec().decode(&payload).unwrap();
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

    // Connect as client using Channel
    let mut channel = Channel::tcp(addr, Codec::Bincode).await.unwrap();

    // Build request
    let add_req = AddRequest { a: 5, b: 3 };
    let payload = channel.codec().encode(&add_req).unwrap();

    // Build RPC header
    let header = constellation_node::rpc::RpcHeader {
        request_id: Uuid::new_v4(),
        route: "MathService.failing.v1".to_string(),
        trace_id: None,
        parent_span_id: None,
    };

    // Send framed request
    channel.send_framed(&header, &payload).await.unwrap();

    // Receive framed response
    let (response_header, response_payload): (constellation_node::rpc::RpcHeader, Vec<u8>) =
        channel.receive_framed().await.unwrap();

    // Verify header
    assert_eq!(response_header.request_id, header.request_id);
    assert_eq!(response_header.route, header.route);

    // Decode RpcResponse
    use constellation_node::rpc::{ErrorCategory, ResponseResult, RpcResponse};
    let rpc_response: RpcResponse = channel.codec().decode(&response_payload).unwrap();
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
            let error: TestError = channel.codec().decode(&payload).unwrap();
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

    // Build and send request with trace context using Channel
    let trace_id = constellation_telemetry::TraceId::new();
    let span_id = constellation_telemetry::SpanId::new();

    let mut channel = Channel::tcp(addr, Codec::Bincode).await.unwrap();

    let header = constellation_node::rpc::RpcHeader {
        request_id: Uuid::new_v4(),
        route: "TelemetryTest.add.v1".to_string(),
        trace_id: Some(trace_id.clone()),
        parent_span_id: Some(span_id.clone()),
    };

    let payload = channel.codec()
        .encode(&AddRequest { a: 10, b: 20 })
        .unwrap();

    // Send framed request
    channel.send_framed(&header, &payload).await.unwrap();

    // Receive framed response
    let (_resp_header, resp_payload): (constellation_node::rpc::RpcHeader, Vec<u8>) =
        channel.receive_framed().await.unwrap();
    let response: constellation_node::rpc::RpcResponse =
        channel.codec().decode(&resp_payload).unwrap();

    // Verify response succeeded
    match response.result {
        constellation_node::rpc::ResponseResult::Success(payload) => {
            let add_response: AddResponse = channel.codec().decode(&payload).unwrap();
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

#[tokio::test]
async fn test_health_endpoints() {
    use constellation_node::health::{HealthStatus, ReadyResponse, StatusResponse};

    // Setup listener
    let listener = TcpTransportListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    // Build node with custom health check
    let node = Node::builder()
        .service_name("HealthTest")
        .id("health-node")
        .auto_discover(false)
        .health_check("always_ok", || async { Ok(()) })
        .binding(
            constellation_node::Binding::new(listener, "tcp")
                .advertise("default", &addr.to_string()),
        )
        .build()
        .unwrap();

    // Spawn node runtime
    tokio::spawn(async move {
        let _ = node.start().await;
    });

    // Give node time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect as client using Channel
    let mut channel = Channel::tcp(addr, Codec::Bincode).await.unwrap();

    // Test _health.status
    {
        let payload = vec![]; // Empty payload - handler takes no request
        let header = constellation_node::rpc::RpcHeader {
            request_id: Uuid::new_v4(),
            route: "_health.status".to_string(),
            trace_id: None,
            parent_span_id: None,
        };

        channel.send_framed(&header, &payload).await.unwrap();
        let (_resp_header, resp_payload): (constellation_node::rpc::RpcHeader, Vec<u8>) =
            channel.receive_framed().await.unwrap();
        let rpc_response: constellation_node::rpc::RpcResponse =
            channel.codec().decode(&resp_payload).unwrap();

        match rpc_response.result {
            constellation_node::rpc::ResponseResult::Success(payload) => {
                let status: StatusResponse = channel.codec()
                    .decode(&payload)
                    .expect("Failed to decode StatusResponse");
                assert_eq!(status.status, HealthStatus::Healthy);
                assert!(
                    status.checks.contains_key("always_ok"),
                    "Should have 'always_ok' check result"
                );
                assert!(status.checks["always_ok"].ok, "always_ok check should pass");
            }
            constellation_node::rpc::ResponseResult::Error { category, payload } => {
                panic!(
                    "Expected success response for _health.status, got error: {:?}, payload len: {}",
                    category,
                    payload.len()
                );
            }
        }
    }

    // Test _health.ready
    {
        let payload = vec![]; // Empty payload - handler takes no request
        let header = constellation_node::rpc::RpcHeader {
            request_id: Uuid::new_v4(),
            route: "_health.ready".to_string(),
            trace_id: None,
            parent_span_id: None,
        };

        channel.send_framed(&header, &payload).await.unwrap();
        let (_resp_header, resp_payload): (constellation_node::rpc::RpcHeader, Vec<u8>) =
            channel.receive_framed().await.unwrap();
        let rpc_response: constellation_node::rpc::RpcResponse =
            channel.codec().decode(&resp_payload).unwrap();

        match rpc_response.result {
            constellation_node::rpc::ResponseResult::Success(payload) => {
                let ready: ReadyResponse = channel.codec().decode(&payload).unwrap();
                assert!(ready.ready, "Node should be ready");
            }
            _ => panic!("Expected success response for _health.ready"),
        }
    }
}

#[tokio::test]
async fn test_health_with_failing_check() {
    use constellation_node::health::{HealthStatus, StatusResponse};

    // Setup listener
    let listener = TcpTransportListener::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    // Build node with a failing health check
    let node = Node::builder()
        .service_name("HealthTest")
        .id("health-fail-node")
        .auto_discover(false)
        .health_check("always_fail", || async {
            Err("Database connection failed".to_string())
        })
        .health_check("always_ok", || async { Ok(()) })
        .binding(
            constellation_node::Binding::new(listener, "tcp")
                .advertise("default", &addr.to_string()),
        )
        .build()
        .unwrap();

    // Spawn node runtime
    tokio::spawn(async move {
        let _ = node.start().await;
    });

    // Give node time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect as client using Channel
    let mut channel = Channel::tcp(addr, Codec::Bincode).await.unwrap();

    // Test _health.status returns degraded
    let payload = vec![]; // Empty payload - handler takes no request
    let header = constellation_node::rpc::RpcHeader {
        request_id: Uuid::new_v4(),
        route: "_health.status".to_string(),
        trace_id: None,
        parent_span_id: None,
    };

    channel.send_framed(&header, &payload).await.unwrap();
    let (_resp_header, resp_payload): (constellation_node::rpc::RpcHeader, Vec<u8>) =
        channel.receive_framed().await.unwrap();
    let rpc_response: constellation_node::rpc::RpcResponse = channel.codec().decode(&resp_payload).unwrap();

    match rpc_response.result {
        constellation_node::rpc::ResponseResult::Success(payload) => {
            let status: StatusResponse = channel.codec()
                .decode(&payload)
                .expect("Failed to decode StatusResponse");
            // Should be Degraded since one check is failing but not all
            assert_eq!(
                status.status,
                HealthStatus::Degraded,
                "Status should be Degraded when one check fails"
            );
            assert!(
                !status.checks["always_fail"].ok,
                "always_fail check should fail"
            );
            assert!(
                status.checks["always_fail"]
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("Database"),
                "Error message should be included"
            );
        }
        constellation_node::rpc::ResponseResult::Error { category, payload } => {
            panic!(
                "Expected success response for _health.status, got error: {:?}, payload len: {}",
                category,
                payload.len()
            );
        }
    }
}
