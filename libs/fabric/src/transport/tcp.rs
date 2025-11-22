use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::error::{Error, Result};
use crate::transport::Transport;

/// TCP transport with length-prefix framing
///
/// Messages are sent with a 4-byte big-endian length prefix
pub struct TcpTransport {
    stream: TcpStream,
    send_timeout: Option<Duration>,
    receive_timeout: Option<Duration>,
}

impl TcpTransport {
    /// Connect to a remote TCP address with no timeouts
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        Self::builder().address(addr).connect().await
    }

    /// Connect with a connect timeout
    pub async fn connect_timeout(addr: SocketAddr, timeout: Duration) -> Result<Self> {
        Self::builder()
            .address(addr)
            .connect_timeout(timeout)
            .connect()
            .await
    }

    /// Create a builder for configuring the transport
    pub fn builder() -> TcpTransportBuilder {
        TcpTransportBuilder::new()
    }

    /// Create from an existing TcpStream
    pub fn from_stream(stream: TcpStream) -> Self {
        Self {
            stream,
            send_timeout: None,
            receive_timeout: None,
        }
    }

    /// Get the remote address of this connection
    pub fn peer_addr(&self) -> Result<SocketAddr> {
        self.stream.peer_addr().map_err(Into::into)
    }

    /// Get the local address of this connection
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.stream.local_addr().map_err(Into::into)
    }
}

#[async_trait::async_trait]
impl Transport for TcpTransport {
    async fn send(&mut self, bytes: &[u8]) -> Result<()> {
        let send_op = async {
            // Write length prefix (4 bytes, big-endian)
            let len = bytes.len() as u32;
            self.stream.write_u32(len).await?;

            // Write data
            self.stream.write_all(bytes).await?;
            self.stream.flush().await?;

            Ok::<(), Error>(())
        };

        if let Some(timeout) = self.send_timeout {
            tokio::time::timeout(timeout, send_op)
                .await
                .map_err(|_| Error::Custom("Send timeout exceeded".to_string()))?
        } else {
            send_op.await
        }
    }

    async fn receive(&mut self) -> Result<Vec<u8>> {
        let receive_op = async {
            // Read length prefix
            let len = self.stream.read_u32().await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    Error::ConnectionClosed
                } else {
                    e.into()
                }
            })? as usize;

            // Validate length (max 100MB to prevent DOS)
            if len > 100 * 1024 * 1024 {
                return Err(Error::InvalidFrame(format!(
                    "Message too large: {} bytes",
                    len
                )));
            }

            // Read data
            let mut buf = vec![0u8; len];
            self.stream.read_exact(&mut buf).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    Error::ConnectionClosed
                } else {
                    e.into()
                }
            })?;

            Ok::<Vec<u8>, Error>(buf)
        };

        if let Some(timeout) = self.receive_timeout {
            tokio::time::timeout(timeout, receive_op)
                .await
                .map_err(|_| Error::Custom("Receive timeout exceeded".to_string()))?
        } else {
            receive_op.await
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.stream.shutdown().await?;
        Ok(())
    }
}

/// TCP listener for accepting incoming connections
pub struct TcpTransportListener {
    listener: TcpListener,
}

impl TcpTransportListener {
    /// Bind to a local address
    pub async fn bind(addr: SocketAddr) -> Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        Ok(Self { listener })
    }

    /// Accept an incoming connection
    pub async fn accept(&self) -> Result<(TcpTransport, SocketAddr)> {
        let (stream, addr) = self.listener.accept().await?;
        Ok((TcpTransport::from_stream(stream), addr))
    }

    /// Get the local address this listener is bound to
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.listener.local_addr().map_err(Into::into)
    }

    /// Close the listener
    ///
    /// Note: Tokio's TcpListener doesn't have an explicit close,
    /// cleanup happens on drop. This is a no-op for compatibility.
    pub async fn close(&mut self) -> Result<()> {
        // TcpListener cleanup happens on drop
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::transport::TransportListener for TcpTransportListener {
    type Transport = TcpTransport;

    async fn accept(&self) -> Result<Self::Transport> {
        let (stream, _) = self.listener.accept().await?;
        Ok(TcpTransport::from_stream(stream))
    }

    async fn close(&mut self) -> Result<()> {
        self.close().await
    }
}

/// Builder for configuring TCP transport
#[derive(Default)]
pub struct TcpTransportBuilder {
    address: Option<SocketAddr>,
    connect_timeout: Option<Duration>,
    send_timeout: Option<Duration>,
    receive_timeout: Option<Duration>,
}

impl TcpTransportBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the address to connect to
    pub fn address(mut self, addr: SocketAddr) -> Self {
        self.address = Some(addr);
        self
    }

    /// Set the connection timeout
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Set the send timeout
    pub fn send_timeout(mut self, timeout: Duration) -> Self {
        self.send_timeout = Some(timeout);
        self
    }

    /// Set the receive timeout
    pub fn receive_timeout(mut self, timeout: Duration) -> Self {
        self.receive_timeout = Some(timeout);
        self
    }

    /// Connect with the configured settings
    pub async fn connect(self) -> Result<TcpTransport> {
        let addr = self
            .address
            .ok_or_else(|| Error::Custom("Address not set".to_string()))?;

        let connect_op = TcpStream::connect(addr);

        let stream = if let Some(timeout) = self.connect_timeout {
            tokio::time::timeout(timeout, connect_op)
                .await
                .map_err(|_| Error::Custom("Connect timeout exceeded".to_string()))??
        } else {
            connect_op.await?
        };

        Ok(TcpTransport {
            stream,
            send_timeout: self.send_timeout,
            receive_timeout: self.receive_timeout,
        })
    }
}
