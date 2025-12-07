//! Tests for binding module

use constellation_fabric::transport::{Transport, TransportListener};
use constellation_fabric::{Codec, Error as FabricError};
use constellation_node::Binding;

// Mock listener for testing
struct MockListener;

#[async_trait::async_trait]
impl TransportListener for MockListener {
    type Transport = MockTransport;

    async fn accept(&self) -> std::result::Result<Self::Transport, FabricError> {
        Ok(MockTransport)
    }

    async fn close(&mut self) -> std::result::Result<(), FabricError> {
        Ok(())
    }
}

struct MockTransport;

#[async_trait::async_trait]
impl Transport for MockTransport {
    fn name(&self) -> &str {
        "mock"
    }

    async fn send(&mut self, _data: &[u8]) -> std::result::Result<(), FabricError> {
        Ok(())
    }

    async fn receive(&mut self) -> std::result::Result<Vec<u8>, FabricError> {
        Ok(vec![])
    }

    async fn close(&mut self) -> std::result::Result<(), FabricError> {
        Ok(())
    }
}

#[test]
fn test_binding_defaults() {
    let binding = Binding::new(MockListener, "tcp");
    assert_eq!(binding.transport(), "tcp");
    assert_eq!(binding.codecs_list(), &[Codec::Bincode]);
    assert!(binding.advertised().is_empty());
    assert!(binding.binding_id().is_empty());
}

#[test]
fn test_binding_with_codecs() {
    let binding =
        Binding::new(MockListener, "tcp").codecs([Codec::Bincode, Codec::Json, Codec::Cbor]);
    assert_eq!(
        binding.codecs_list(),
        &[Codec::Bincode, Codec::Json, Codec::Cbor]
    );
}

#[test]
fn test_binding_advertise() {
    let binding = Binding::new(MockListener, "tcp")
        .advertise("internal", "10.0.1.5:8080")
        .advertise("external", "203.0.113.5:8080");

    assert_eq!(binding.advertised().len(), 2);
    assert_eq!(binding.advertised()[0].network, "internal");
    assert_eq!(binding.advertised()[0].address, "10.0.1.5:8080");
    assert_eq!(binding.advertised()[1].network, "external");
    assert_eq!(binding.advertised()[1].address, "203.0.113.5:8080");
}

#[test]
fn test_binding_id_generated_from_first_address() {
    let binding = Binding::new(MockListener, "tcp")
        .advertise("internal", "10.0.1.5:8080")
        .advertise("external", "203.0.113.5:8080");

    assert_eq!(binding.binding_id(), "tcp_10.0.1.5:8080");
}
