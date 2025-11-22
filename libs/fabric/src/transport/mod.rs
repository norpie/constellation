use crate::error::Result;

pub mod tcp;
pub mod unix;

pub use self::tcp::{TcpTransport, TcpTransportBuilder, TcpTransportListener};
pub use self::unix::{UnixTransport, UnixTransportBuilder, UnixTransportListener};

/// Transport trait for sending and receiving raw bytes
///
/// Each transport instance represents a single connection.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Send bytes over the transport
    async fn send(&mut self, bytes: &[u8]) -> Result<()>;

    /// Receive bytes from the transport
    async fn receive(&mut self) -> Result<Vec<u8>>;

    /// Close the transport connection
    async fn close(&mut self) -> Result<()>;
}

/// Listener trait for accepting incoming connections
///
/// Provides a unified interface for server-side transport listeners.
/// Each listener produces its own transport type with transport-specific
/// peer information accessible via methods on the transport itself.
#[async_trait::async_trait]
pub trait TransportListener: Send + Sync {
    /// The transport type this listener produces
    type Transport: Transport;

    /// Accept an incoming connection
    async fn accept(&self) -> Result<Self::Transport>;

    /// Close the listener gracefully
    async fn close(&mut self) -> Result<()>;
}
