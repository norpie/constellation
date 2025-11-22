use std::net::SocketAddr;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::codec::Codec;
use crate::error::Result;
use crate::transport::{TcpTransport, Transport, UnixTransport};

/// High-level channel for bidirectional communication
///
/// Combines a transport and codec for persistent connections
pub struct Channel<C> {
    transport: Box<dyn Transport>,
    codec: C,
}

impl<C: Codec> Channel<C> {
    /// Create a channel from an existing transport
    pub fn from_transport(transport: impl Transport + 'static, codec: C) -> Self {
        Self {
            transport: Box::new(transport),
            codec,
        }
    }

    /// Open a TCP channel
    pub async fn tcp(addr: SocketAddr, codec: C) -> Result<Self> {
        let transport = TcpTransport::connect(addr).await?;
        Ok(Self::from_transport(transport, codec))
    }

    /// Open a Unix socket channel
    pub async fn unix(path: impl AsRef<Path>, codec: C) -> Result<Self> {
        let transport = UnixTransport::connect(path).await?;
        Ok(Self::from_transport(transport, codec))
    }

    /// Send a message over the channel
    pub async fn send<T: Serialize>(&mut self, message: &T) -> Result<()> {
        let bytes = self.codec.encode(message)?;
        self.transport.send(&bytes).await
    }

    /// Receive a message from the channel
    pub async fn receive<T: for<'de> Deserialize<'de>>(&mut self) -> Result<T> {
        let bytes = self.transport.receive().await?;
        self.codec.decode(&bytes)
    }

    /// Close the channel
    pub async fn close(mut self) -> Result<()> {
        self.transport.close().await
    }
}
