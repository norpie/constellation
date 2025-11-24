// Test the #[handler] macro

use constellation_node::handler::Handler;
use constellation_node::rpc::RpcRequest;
use constellation_node::{handler, Data, Node};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct EchoRequest {
    message: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct EchoResponse {
    echo: String,
    prefix: String,
}

#[derive(Debug)]
struct MyError(String);

impl std::fmt::Display for MyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MyError {}

#[handler]
async fn echo(req: EchoRequest, prefix: Data<String>) -> Result<EchoResponse, MyError> {
    Ok(EchoResponse {
        echo: req.message,
        prefix: prefix.to_string(),
    })
}

#[tokio::test]
async fn test_handler_macro_basic() {
    use constellation_fabric::codec::{BincodeCodec, Codec};

    // Build node
    let node = Node::builder()
        .service_name("TestService")
        .data("Hello".to_string())
        .build()
        .unwrap();

    // Create request
    let codec = BincodeCodec;
    let echo_req = EchoRequest {
        message: "world".to_string(),
    };
    let payload = codec.encode(&echo_req).unwrap();

    let request = RpcRequest {
        request_id: Uuid::new_v4(),
        route: "TestService.echo.v1".to_string(),
        payload,
    };

    // Call handler
    let handler = &ECHO_HANDLER;
    let response_bytes = handler.call(&node, &request).await.unwrap();

    // Decode response
    let response: EchoResponse = codec.decode(&response_bytes).unwrap();
    assert_eq!(response.echo, "world");
    assert_eq!(response.prefix, "Hello");
}

#[handler(version = 2)]
async fn echo_v2(req: EchoRequest, prefix: Data<String>) -> Result<EchoResponse, MyError> {
    Ok(EchoResponse {
        echo: format!("{}!", req.message),
        prefix: prefix.to_string(),
    })
}

#[tokio::test]
async fn test_handler_macro_with_version() {
    // Verify the version is set correctly
    // The handler should be registered with version 2
    // We'll just verify it compiles and the constant exists
    let _handler = &ECHO_V2_HANDLER;
}

#[derive(Debug, Serialize, Deserialize)]
struct CallOtherServiceRequest {
    user_id: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CallOtherServiceResponse {
    result: String,
}

// Handler that extracts RpcClient to make calls to other services
#[handler]
async fn call_other_service(
    req: CallOtherServiceRequest,
    _rpc: Data<constellation_node::RpcClient>,
) -> Result<CallOtherServiceResponse, MyError> {
    // In a real handler, you would use rpc to make calls to other services
    // For now, just verify we can extract it
    Ok(CallOtherServiceResponse {
        result: format!("Would call other service for user {}", req.user_id),
    })
}

#[tokio::test]
async fn test_handler_with_rpc_client() {
    use constellation_fabric::codec::{BincodeCodec, Codec};

    // Build node - RpcClient is auto-registered
    let node = Node::builder()
        .service_name("TestService")
        .build()
        .unwrap();

    // Create request
    let codec = BincodeCodec;
    let req = CallOtherServiceRequest { user_id: 123 };
    let payload = codec.encode(&req).unwrap();

    let request = RpcRequest {
        request_id: Uuid::new_v4(),
        route: "TestService.call_other_service.v1".to_string(),
        payload,
    };

    // Call handler - it should be able to extract RpcClient
    let handler = &CALL_OTHER_SERVICE_HANDLER;
    let response_bytes = handler.call(&node, &request).await.unwrap();

    // Decode response
    let response: CallOtherServiceResponse = codec.decode(&response_bytes).unwrap();
    assert_eq!(response.result, "Would call other service for user 123");
}
