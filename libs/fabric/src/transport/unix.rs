use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::error::{Error, Result};
use crate::transport::Transport;

/// Unix domain socket transport with length-prefix framing
///
/// Messages are sent with a 4-byte big-endian length prefix
pub struct UnixTransport {
    stream: UnixStream,
    send_timeout: Option<Duration>,
    receive_timeout: Option<Duration>,
}

impl UnixTransport {
    /// Connect to a Unix socket with no timeouts
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self> {
        Self::builder().path(path).connect().await
    }

    /// Connect with a connect timeout
    pub async fn connect_timeout(path: impl AsRef<Path>, timeout: Duration) -> Result<Self> {
        Self::builder()
            .path(path)
            .connect_timeout(timeout)
            .connect()
            .await
    }

    /// Create a builder for configuring the transport
    pub fn builder() -> UnixTransportBuilder {
        UnixTransportBuilder::new()
    }

    /// Create from an existing UnixStream
    pub fn from_stream(stream: UnixStream) -> Self {
        Self {
            stream,
            send_timeout: None,
            receive_timeout: None,
        }
    }
}

#[async_trait::async_trait]
impl Transport for UnixTransport {
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

/// Unix socket listener for accepting incoming connections
pub struct UnixTransportListener {
    listener: UnixListener,
    path: PathBuf,
}

impl UnixTransportListener {
    /// Bind to a Unix socket path
    pub async fn bind(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Remove existing socket file if it exists
        if path.exists() {
            std::fs::remove_file(&path)?;
        }

        let listener = UnixListener::bind(&path)?;
        Ok(Self { listener, path })
    }

    /// Accept an incoming connection
    pub async fn accept(&self) -> Result<UnixTransport> {
        let (stream, _) = self.listener.accept().await?;
        Ok(UnixTransport::from_stream(stream))
    }

    /// Get the path this listener is bound to
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Close the listener and remove the socket file
    pub async fn close(&mut self) -> Result<()> {
        std::fs::remove_file(&self.path)?;
        Ok(())
    }
}

impl Drop for UnixTransportListener {
    fn drop(&mut self) {
        // Clean up socket file on drop
        let _ = std::fs::remove_file(&self.path);
    }
}

#[async_trait::async_trait]
impl crate::transport::TransportListener for UnixTransportListener {
    type Transport = UnixTransport;

    async fn accept(&self) -> Result<Self::Transport> {
        let (stream, _) = self.listener.accept().await?;
        Ok(UnixTransport::from_stream(stream))
    }

    async fn close(&mut self) -> Result<()> {
        self.close().await
    }
}

/// Builder for configuring Unix socket transport
#[derive(Default)]
pub struct UnixTransportBuilder {
    path: Option<PathBuf>,
    connect_timeout: Option<Duration>,
    send_timeout: Option<Duration>,
    receive_timeout: Option<Duration>,
}

impl UnixTransportBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the path to connect to
    pub fn path(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().to_path_buf());
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
    pub async fn connect(self) -> Result<UnixTransport> {
        let path = self
            .path
            .ok_or_else(|| Error::Custom("Path not set".to_string()))?;

        let connect_op = UnixStream::connect(path);

        let stream = if let Some(timeout) = self.connect_timeout {
            tokio::time::timeout(timeout, connect_op)
                .await
                .map_err(|_| Error::Custom("Connect timeout exceeded".to_string()))??
        } else {
            connect_op.await?
        };

        Ok(UnixTransport {
            stream,
            send_timeout: self.send_timeout,
            receive_timeout: self.receive_timeout,
        })
    }
}
