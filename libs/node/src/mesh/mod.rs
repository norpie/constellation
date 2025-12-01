// Mesh discovery and transponder data types

mod address_book;

pub use address_book::{AddressBook, AddressBookCommand, AddressBookResponse};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Response to a mesh join request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JoinResponse {
    /// Join successful - node is being added to cluster
    Success,
    /// Not the leader - try the suggested node instead
    NotLeader { leader: Option<TransponderData> },
}

/// Data structure advertised when a node joins the mesh
///
/// Contains everything other nodes need to know to communicate with this node:
/// - Where to reach it (addresses)
/// - What protocols it speaks (transports, codecs)
/// - What services it provides (routes)
/// - What constraints it has (security, performance)
/// - What special capabilities it has (forwarding, translation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransponderData {
    pub node_id: String,
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

/// Group of addresses for a specific transport in a specific zone
///
/// A node can advertise multiple address groups for different network zones
/// (e.g., internal vs external) and different transports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressGroup {
    pub zone: String,
    pub transport: String,
    pub addresses: Vec<String>,
}

impl AddressGroup {
    /// Create a builder for AddressGroup
    pub fn builder() -> AddressGroupBuilder {
        AddressGroupBuilder::new()
    }

    /// Create an AddressGroup with a single address
    pub fn single(zone: impl Into<String>, transport: impl Into<String>, address: impl Into<String>) -> Self {
        Self {
            zone: zone.into(),
            transport: transport.into(),
            addresses: vec![address.into()],
        }
    }
}

/// Builder for AddressGroup
#[derive(Default)]
pub struct AddressGroupBuilder {
    zone: Option<String>,
    transport: Option<String>,
    addresses: Vec<String>,
}

impl AddressGroupBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn zone(mut self, zone: impl Into<String>) -> Self {
        self.zone = Some(zone.into());
        self
    }

    pub fn transport(mut self, transport: impl Into<String>) -> Self {
        self.transport = Some(transport.into());
        self
    }

    pub fn address(mut self, address: impl Into<String>) -> Self {
        self.addresses.push(address.into());
        self
    }

    pub fn addresses(mut self, addresses: Vec<String>) -> Self {
        self.addresses = addresses;
        self
    }

    pub fn build(self) -> AddressGroup {
        AddressGroup {
            zone: self.zone.expect("zone is required"),
            transport: self.transport.expect("transport is required"),
            addresses: self.addresses,
        }
    }
}

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
