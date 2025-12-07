use std::net::SocketAddr;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::codec::Codec;
use crate::error::Result;
use crate::transport::{TcpTransport, Transport, UnixTransport};

/// High-level channel for bidirectional communication
///
/// Combines a transport and codec for persistent connections
pub struct Channel {
    transport: Box<dyn Transport>,
    codec: Codec,
}

impl Channel {
    /// Create a channel from an existing transport
    pub fn from_transport(transport: impl Transport + 'static, codec: Codec) -> Self {
        Self {
            transport: Box::new(transport),
            codec,
        }
    }

    /// Create a channel from an already-boxed transport
    pub fn from_transport_boxed(transport: Box<dyn Transport>, codec: Codec) -> Self {
        Self { transport, codec }
    }

    /// Open a TCP channel
    pub async fn tcp(addr: SocketAddr, codec: Codec) -> Result<Self> {
        let transport = TcpTransport::connect(addr).await?;
        Ok(Self::from_transport(transport, codec))
    }

    /// Open a Unix socket channel
    pub async fn unix(path: impl AsRef<Path>, codec: Codec) -> Result<Self> {
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

    /// Get a reference to the channel's codec
    pub fn codec(&self) -> &Codec {
        &self.codec
    }

    /// Send a framed message with header and payload
    ///
    /// Frame format: `[header_len: u32][header_bytes][payload_bytes]`
    ///
    /// The header is encoded using the channel's codec. The payload is sent as-is
    /// (typically pre-encoded by the caller to avoid double-serialization).
    pub async fn send_framed<H: Serialize>(&mut self, header: &H, payload: &[u8]) -> Result<()> {
        let header_bytes = self.codec.encode(header)?;
        let header_len = header_bytes.len() as u32;

        // Build frame: [header_len: u32][header_bytes][payload_bytes]
        let total_len = 4 + header_bytes.len() + payload.len();
        let mut frame = Vec::with_capacity(total_len);
        frame.extend_from_slice(&header_len.to_be_bytes());
        frame.extend_from_slice(&header_bytes);
        frame.extend_from_slice(payload);

        self.transport.send(&frame).await
    }

    /// Receive a framed message, returning decoded header and raw payload
    ///
    /// Frame format: `[header_len: u32][header_bytes][payload_bytes]`
    ///
    /// The header is decoded using the channel's codec. The payload is returned
    /// as raw bytes for the caller to decode (avoiding double-deserialization).
    pub async fn receive_framed<H: for<'de> Deserialize<'de>>(&mut self) -> Result<(H, Vec<u8>)> {
        let frame = self.transport.receive().await?;

        // Parse header length
        if frame.len() < 4 {
            return Err(crate::error::Error::Custom("Frame too short".to_string()));
        }
        let header_len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;

        // Validate frame length
        if frame.len() < 4 + header_len {
            return Err(crate::error::Error::Custom("Incomplete frame".to_string()));
        }

        // Decode header
        let header_bytes = &frame[4..4 + header_len];
        let header: H = self.codec.decode(header_bytes)?;

        // Extract payload
        let payload = frame[4 + header_len..].to_vec();

        Ok((header, payload))
    }
}
