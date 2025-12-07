// Binding - ties a listener to codecs and advertised addresses

use crate::error::Result;
use constellation_fabric::transport::{Transport, TransportListener};
use constellation_fabric::Codec;

/// Object-safe wrapper for TransportListener (internal use)
///
/// Allows storing heterogeneous listeners in NodeBuilder
#[async_trait::async_trait]
pub(crate) trait ListenerHandle: Send + Sync {
    /// Accept a connection and return a boxed Transport
    async fn accept_connection(&self) -> Result<Box<dyn Transport>>;
}

/// A network endpoint advertised to other nodes
#[derive(Debug, Clone)]
pub struct AdvertisedEndpoint {
    /// Network classification (e.g., "internal", "external", "dmz")
    pub network: String,
    /// The address other nodes can use to connect
    pub address: String,
}

/// Binds a transport listener to codecs and advertised addresses
///
/// A Binding represents a single listening endpoint that can be advertised
/// on multiple networks with different addresses (e.g., internal IP vs external IP).
///
/// # Example
/// ```ignore
/// use constellation_fabric::transport::TcpTransportListener;
/// use constellation_fabric::Codec;
///
/// let listener = TcpTransportListener::bind("0.0.0.0:8080".parse()?).await?;
///
/// let binding = Binding::new(listener, "tcp")
///     .codecs([Codec::Bincode, Codec::Json])
///     .advertise("internal", "10.0.1.5:8080")
///     .advertise("external", "203.0.113.5:8080");
///
/// Node::builder()
///     .service_name("MyService")
///     .binding(binding)
///     .build()
/// ```
pub struct Binding {
    pub(crate) listener: Box<dyn ListenerHandle>,
    pub(crate) transport: String,
    pub(crate) codecs: Vec<Codec>,
    pub(crate) advertised: Vec<AdvertisedEndpoint>,
    pub(crate) binding_id: String,
}

impl Binding {
    /// Create a new Binding for a transport listener
    ///
    /// # Arguments
    /// * `listener` - Any type implementing `TransportListener`
    /// * `transport` - Transport protocol name (e.g., "tcp", "unix")
    ///
    /// Codecs default to `[Codec::Bincode]`. Use `.codecs()` to override.
    pub fn new<L>(listener: L, transport: impl Into<String>) -> Self
    where
        L: TransportListener + Send + Sync + 'static,
        L::Transport: Transport + Send + Sync + 'static,
    {
        let transport = transport.into();

        Self {
            listener: Box::new(ListenerWrapper(listener)),
            transport,
            codecs: vec![Codec::Bincode],
            advertised: Vec::new(),
            binding_id: String::new(), // Will be generated when first address is advertised
        }
    }

    /// Set the codecs supported by this binding
    ///
    /// Replaces the default `[Codec::Bincode]`.
    pub fn codecs<I>(mut self, codecs: I) -> Self
    where
        I: IntoIterator<Item = Codec>,
    {
        self.codecs = codecs.into_iter().collect();
        self
    }

    /// Advertise this binding on a network with the given address
    ///
    /// Can be called multiple times to advertise on multiple networks.
    ///
    /// # Arguments
    /// * `network` - Network classification (e.g., "internal", "external", "dmz")
    /// * `address` - The address other nodes can use to connect
    pub fn advertise(mut self, network: impl Into<String>, address: impl Into<String>) -> Self {
        let address = address.into();

        // Generate binding_id from first advertised address
        if self.binding_id.is_empty() {
            self.binding_id = format!("{}_{}", self.transport, address);
        }

        self.advertised.push(AdvertisedEndpoint {
            network: network.into(),
            address,
        });
        self
    }

    /// Get the binding ID
    ///
    /// Auto-generated from transport + first advertised address.
    /// Used for correlating health status with specific listeners.
    pub fn binding_id(&self) -> &str {
        &self.binding_id
    }

    /// Get the transport name
    pub fn transport(&self) -> &str {
        &self.transport
    }

    /// Get the supported codecs
    pub fn codecs_list(&self) -> &[Codec] {
        &self.codecs
    }

    /// Get the advertised endpoints
    pub fn advertised(&self) -> &[AdvertisedEndpoint] {
        &self.advertised
    }
}

/// Wrapper that implements ListenerHandle for any TransportListener
pub struct ListenerWrapper<L>(pub L);

#[async_trait::async_trait]
impl<L> ListenerHandle for ListenerWrapper<L>
where
    L: TransportListener + Send + Sync,
    L::Transport: Transport + Send + Sync + 'static,
{
    async fn accept_connection(&self) -> crate::error::Result<Box<dyn Transport>> {
        let transport = self.0.accept().await?;
        Ok(Box::new(transport))
    }
}
