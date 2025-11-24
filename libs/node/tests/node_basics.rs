// Basic node tests

use constellation_fabric::codec::{BincodeCodec, Codec};
use constellation_node::handler::Handler;
use constellation_node::rpc::RpcRequest;
use constellation_node::{Data, Node};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Simple test request/response types
#[derive(Debug, Serialize, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct LoginResponse {
    token: String,
    user_id: u64,
}

// Simple test handler
struct LoginHandler;

#[async_trait::async_trait]
impl Handler for LoginHandler {
    async fn call(
        &self,
        node: &Node,
        request: &RpcRequest,
    ) -> constellation_node::Result<Vec<u8>> {
        // Decode request using bincode
        let codec = BincodeCodec;
        let req: LoginRequest = codec
            .decode(&request.payload)
            .map_err(|e| constellation_node::Error::Serialization(e.to_string()))?;

        // Extract data (in real handler, this would be db connection etc.)
        let greeting: Data<String> = node
            .extract()
            .ok_or_else(|| constellation_node::Error::MissingDependency("String".to_string()))?;

        // Simple logic
        let response = LoginResponse {
            token: format!("{}-{}-token", greeting.as_str(), req.username),
            user_id: 42,
        };

        // Encode response
        codec
            .encode(&response)
            .map_err(|e| constellation_node::Error::Serialization(e.to_string()))
    }
}

#[tokio::test]
async fn test_manual_handler_registration() {
    // Build node with manual handler registration
    let greeting = "Hello".to_string();

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(greeting)
        .register("login.v1", &LoginHandler)
        .build()
        .unwrap();

    // Verify service name
    assert_eq!(node.service_name(), "TestService");

    // Verify data extraction works
    let extracted: Data<String> = node.extract().unwrap();
    assert_eq!(extracted.as_str(), "Hello");
}

#[tokio::test]
async fn test_data_extraction() {
    struct Config {
        api_key: String,
    }

    let config = Config {
        api_key: "secret123".to_string(),
    };

    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .data(config)
        .data(42u64) // Multiple data types
        .build()
        .unwrap();

    // Extract both types
    let config: Data<Config> = node.extract().unwrap();
    assert_eq!(config.api_key, "secret123");

    let number: Data<u64> = node.extract().unwrap();
    assert_eq!(*number, 42);

    // Non-existent type returns None
    let missing: Option<Data<String>> = node.extract();
    assert!(missing.is_none());
}

#[tokio::test]
async fn test_service_name_required() {
    let result = Node::builder().build();

    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(err.to_string().contains("Service name is required"));
}

#[tokio::test]
async fn test_handler_call() {
    let node = Node::builder()
        .service_name("AuthService")
        .auto_discover(false)
        .data("Welcome".to_string())
        .register("login.v1", &LoginHandler)
        .build()
        .unwrap();

    // Create request
    let codec = BincodeCodec;
    let login_req = LoginRequest {
        username: "alice".to_string(),
        password: "secret".to_string(),
    };
    let payload = codec.encode(&login_req).unwrap();

    let request = RpcRequest {
        request_id: Uuid::new_v4(),
        route: "AuthService.login.v1".to_string(),
        payload,
    };

    // Call handler
    let handler = &LoginHandler;
    let response_bytes = handler.call(&node, &request).await.unwrap();

    // Decode response
    let response: LoginResponse = codec.decode(&response_bytes).unwrap();
    assert_eq!(response.token, "Welcome-alice-token");
    assert_eq!(response.user_id, 42);
}

#[tokio::test]
async fn test_rpc_client_auto_registered() {
    use constellation_node::RpcClient;

    // Build a node
    let node = Node::builder()
        .service_name("TestService")
        .auto_discover(false)
        .build()
        .unwrap();

    // Verify RpcClient is automatically registered
    let rpc: Data<RpcClient> = node
        .extract()
        .expect("RpcClient should be auto-registered");

    // Verify we can create a call (even though it won't succeed yet)
    let result = rpc
        .call::<(), ()>("SomeService.method.v1", &())
        .expect("Serialization should succeed")
        .await;

    // Should fail with "not implemented" error since runtime isn't ready
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not yet implemented"));
}
