// Mesh discovery and transponder data types

mod address_book;

pub use address_book::{AddressBook, AddressBookCommand, AddressBookResponse};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Response to mesh operations (join, leave)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshResponse {
    /// Operation successful
    Success,
    /// Not the leader - try the suggested node instead
    NotLeader { leader: Option<TransponderData> },
}

/// Request to leave the mesh
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaveRequest {
    pub node_id: String,
}

/// Data structure advertised when a node joins the mesh
///
/// Contains everything other nodes need to know to communicate with this node:
/// - Where to reach it (addresses)
/// - What protocols it speaks (transports, codecs)
/// - What services it provides (routes)
/// - What constraints it has (security, performance)
/// - What special capabilities it has (forwarding, translation)
/// - Topology information (region, zone) for routing decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransponderData {
    pub node_id: String,
    /// Geographic region (e.g., "us-east", "eu-west", "global")
    pub region: String,
    /// Availability zone within region (e.g., "us-east-1a", "global")
    pub zone: String,
    pub addresses: Vec<AddressGroup>,
    pub transports: Vec<String>,
    pub codecs: Vec<String>,
    pub routes: Vec<String>,
    pub global_constraints: Constraint,
    pub route_constraints: HashMap<String, Constraint>,
    pub capabilities: Capabilities,
}

impl TransponderData {
    /// Create a builder for TransponderData
    pub fn builder() -> TransponderDataBuilder {
        TransponderDataBuilder::new()
    }
}

/// Builder for TransponderData
#[derive(Default)]
pub struct TransponderDataBuilder {
    node_id: Option<String>,
    region: Option<String>,
    zone: Option<String>,
    addresses: Vec<AddressGroup>,
    transports: Vec<String>,
    codecs: Vec<String>,
    routes: Vec<String>,
    global_constraints: Option<Constraint>,
    route_constraints: HashMap<String, Constraint>,
    capabilities: Option<Capabilities>,
}

impl TransponderDataBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn node_id(mut self, id: impl Into<String>) -> Self {
        self.node_id = Some(id.into());
        self
    }

    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    pub fn zone(mut self, zone: impl Into<String>) -> Self {
        self.zone = Some(zone.into());
        self
    }

    pub fn address(mut self, address: AddressGroup) -> Self {
        self.addresses.push(address);
        self
    }

    pub fn addresses(mut self, addresses: Vec<AddressGroup>) -> Self {
        self.addresses = addresses;
        self
    }

    pub fn transport(mut self, transport: impl Into<String>) -> Self {
        self.transports.push(transport.into());
        self
    }

    pub fn transports(mut self, transports: Vec<String>) -> Self {
        self.transports = transports;
        self
    }

    pub fn codec(mut self, codec: impl Into<String>) -> Self {
        self.codecs.push(codec.into());
        self
    }

    pub fn codecs(mut self, codecs: Vec<String>) -> Self {
        self.codecs = codecs;
        self
    }

    pub fn route(mut self, route: impl Into<String>) -> Self {
        self.routes.push(route.into());
        self
    }

    pub fn routes(mut self, routes: Vec<String>) -> Self {
        self.routes = routes;
        self
    }

    pub fn global_constraints(mut self, constraints: Constraint) -> Self {
        self.global_constraints = Some(constraints);
        self
    }

    pub fn route_constraint(mut self, route: impl Into<String>, constraint: Constraint) -> Self {
        self.route_constraints.insert(route.into(), constraint);
        self
    }

    pub fn capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = Some(capabilities);
        self
    }

    pub fn build(self) -> TransponderData {
        TransponderData {
            node_id: self.node_id.expect("node_id is required"),
            region: self.region.unwrap_or_else(|| "global".to_string()),
            zone: self.zone.unwrap_or_else(|| "global".to_string()),
            addresses: self.addresses,
            transports: self.transports,
            codecs: self.codecs,
            routes: self.routes,
            global_constraints: self.global_constraints.unwrap_or_default(),
            route_constraints: self.route_constraints,
            capabilities: self.capabilities.unwrap_or_default(),
        }
    }
}

/// An advertised address for reaching a node
///
/// Each AdvertisedAddress represents a single way to reach a node:
/// a specific address on a specific network using a specific transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvertisedAddress {
    /// Network classification (e.g., "internal", "external", "dmz")
    pub network: String,
    /// Transport protocol (e.g., "tcp", "unix")
    pub transport: String,
    /// The address to connect to
    pub address: String,
    /// Codecs supported on this address
    pub codecs: Vec<constellation_fabric::Codec>,
    /// Binding ID for health correlation
    pub binding_id: String,
}

impl AdvertisedAddress {
    /// Create a builder for AdvertisedAddress
    pub fn builder() -> AdvertisedAddressBuilder {
        AdvertisedAddressBuilder::new()
    }

    /// Create an AdvertisedAddress with minimal fields
    pub fn new(
        network: impl Into<String>,
        transport: impl Into<String>,
        address: impl Into<String>,
    ) -> Self {
        Self {
            network: network.into(),
            transport: transport.into(),
            address: address.into(),
            codecs: vec![constellation_fabric::Codec::Bincode],
            binding_id: String::new(),
        }
    }
}

/// Builder for AdvertisedAddress
#[derive(Default)]
pub struct AdvertisedAddressBuilder {
    network: Option<String>,
    transport: Option<String>,
    address: Option<String>,
    codecs: Vec<constellation_fabric::Codec>,
    binding_id: Option<String>,
}

impl AdvertisedAddressBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn network(mut self, network: impl Into<String>) -> Self {
        self.network = Some(network.into());
        self
    }

    pub fn transport(mut self, transport: impl Into<String>) -> Self {
        self.transport = Some(transport.into());
        self
    }

    pub fn address(mut self, address: impl Into<String>) -> Self {
        self.address = Some(address.into());
        self
    }

    pub fn codecs(mut self, codecs: Vec<constellation_fabric::Codec>) -> Self {
        self.codecs = codecs;
        self
    }

    pub fn codec(mut self, codec: constellation_fabric::Codec) -> Self {
        self.codecs.push(codec);
        self
    }

    pub fn binding_id(mut self, binding_id: impl Into<String>) -> Self {
        self.binding_id = Some(binding_id.into());
        self
    }

    pub fn build(self) -> AdvertisedAddress {
        AdvertisedAddress {
            network: self.network.expect("network is required"),
            transport: self.transport.expect("transport is required"),
            address: self.address.expect("address is required"),
            codecs: if self.codecs.is_empty() {
                vec![constellation_fabric::Codec::Bincode]
            } else {
                self.codecs
            },
            binding_id: self.binding_id.unwrap_or_default(),
        }
    }
}

// Type alias for backwards compatibility during refactor
pub type AddressGroup = AdvertisedAddress;

/// Constraints on which transports and codecs can be used
///
/// Used to enforce security policies, performance requirements, or compatibility.
/// Route-specific constraints override global constraints.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Constraint {
    pub allow_transports: Vec<String>,
    pub deny_transports: Vec<String>,
    pub allow_codecs: Vec<String>,
    pub deny_codecs: Vec<String>,
    pub allow_combinations: Vec<(String, String)>,
    pub deny_combinations: Vec<(String, String)>,
}

impl Constraint {
    /// Create a builder for Constraint
    pub fn builder() -> ConstraintBuilder {
        ConstraintBuilder::new()
    }

    /// Allow all transports and codecs (no restrictions)
    pub fn allow_all() -> Self {
        Self::default()
    }

    /// Deny everything (node cannot be reached)
    pub fn deny_all() -> Self {
        Self {
            allow_transports: vec![],
            deny_transports: vec![],
            allow_codecs: vec![],
            deny_codecs: vec![],
            allow_combinations: vec![],
            deny_combinations: vec![],
        }
    }
}

/// Builder for Constraint
#[derive(Default)]
pub struct ConstraintBuilder {
    allow_transports: Vec<String>,
    deny_transports: Vec<String>,
    allow_codecs: Vec<String>,
    deny_codecs: Vec<String>,
    allow_combinations: Vec<(String, String)>,
    deny_combinations: Vec<(String, String)>,
}

impl ConstraintBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_transport(mut self, transport: impl Into<String>) -> Self {
        self.allow_transports.push(transport.into());
        self
    }

    pub fn allow_transports(mut self, transports: Vec<String>) -> Self {
        self.allow_transports = transports;
        self
    }

    pub fn deny_transport(mut self, transport: impl Into<String>) -> Self {
        self.deny_transports.push(transport.into());
        self
    }

    pub fn deny_transports(mut self, transports: Vec<String>) -> Self {
        self.deny_transports = transports;
        self
    }

    pub fn allow_codec(mut self, codec: impl Into<String>) -> Self {
        self.allow_codecs.push(codec.into());
        self
    }

    pub fn allow_codecs(mut self, codecs: Vec<String>) -> Self {
        self.allow_codecs = codecs;
        self
    }

    pub fn deny_codec(mut self, codec: impl Into<String>) -> Self {
        self.deny_codecs.push(codec.into());
        self
    }

    pub fn deny_codecs(mut self, codecs: Vec<String>) -> Self {
        self.deny_codecs = codecs;
        self
    }

    pub fn allow_combination(mut self, transport: impl Into<String>, codec: impl Into<String>) -> Self {
        self.allow_combinations.push((transport.into(), codec.into()));
        self
    }

    pub fn deny_combination(mut self, transport: impl Into<String>, codec: impl Into<String>) -> Self {
        self.deny_combinations.push((transport.into(), codec.into()));
        self
    }

    pub fn build(self) -> Constraint {
        Constraint {
            allow_transports: self.allow_transports,
            deny_transports: self.deny_transports,
            allow_codecs: self.allow_codecs,
            deny_codecs: self.deny_codecs,
            allow_combinations: self.allow_combinations,
            deny_combinations: self.deny_combinations,
        }
    }
}

/// Capabilities of a node in the mesh
///
/// Describes what advanced features this node supports beyond basic RPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub can_forward: bool,
    pub can_translate: bool,
    pub max_hops: Option<u8>,
}

impl Capabilities {
    /// Create a builder for Capabilities
    pub fn builder() -> CapabilitiesBuilder {
        CapabilitiesBuilder::new()
    }

    /// Basic node capabilities (no forwarding or translation)
    pub fn basic() -> Self {
        Self {
            can_forward: false,
            can_translate: false,
            max_hops: None,
        }
    }

    /// Full capabilities (can forward and translate, unlimited hops)
    pub fn full() -> Self {
        Self {
            can_forward: true,
            can_translate: true,
            max_hops: None,
        }
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::basic()
    }
}

/// Builder for Capabilities
#[derive(Default)]
pub struct CapabilitiesBuilder {
    can_forward: bool,
    can_translate: bool,
    max_hops: Option<u8>,
}

impl CapabilitiesBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn can_forward(mut self, can_forward: bool) -> Self {
        self.can_forward = can_forward;
        self
    }

    pub fn can_translate(mut self, can_translate: bool) -> Self {
        self.can_translate = can_translate;
        self
    }

    pub fn max_hops(mut self, max_hops: u8) -> Self {
        self.max_hops = Some(max_hops);
        self
    }

    pub fn build(self) -> Capabilities {
        Capabilities {
            can_forward: self.can_forward,
            can_translate: self.can_translate,
            max_hops: self.max_hops,
        }
    }
}
