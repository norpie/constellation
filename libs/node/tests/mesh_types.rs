// Tests for mesh types and builders

use constellation_node::mesh::{AdvertisedAddress, Capabilities, Constraint, TransponderData};

#[test]
fn test_transponder_data_builder() {
    let data = TransponderData::builder()
        .node_id("test-node-1")
        .transport("tcp")
        .transport("unix")
        .codec("bincode")
        .route("UserService.login.v1")
        .route("UserService.logout.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:8080"))
        .capabilities(Capabilities::basic())
        .build();

    assert_eq!(data.node_id, "test-node-1");
    assert_eq!(data.transports, vec!["tcp", "unix"]);
    assert_eq!(data.codecs, vec!["bincode"]);
    assert_eq!(data.routes.len(), 2);
    assert_eq!(data.addresses.len(), 1);
    assert!(!data.capabilities.can_forward);
}

#[test]
fn test_advertised_address_builder() {
    let addr = AdvertisedAddress::builder()
        .network("dc-east")
        .transport("tcp")
        .address("10.0.1.5:8080")
        .build();

    assert_eq!(addr.network, "dc-east");
    assert_eq!(addr.transport, "tcp");
    assert_eq!(addr.address, "10.0.1.5:8080");
}

#[test]
fn test_advertised_address_new() {
    let addr = AdvertisedAddress::new("public", "tcp", "203.0.113.5:8080");

    assert_eq!(addr.network, "public");
    assert_eq!(addr.transport, "tcp");
    assert_eq!(addr.address, "203.0.113.5:8080");
}

#[test]
fn test_connection_rules() {
    use constellation_fabric::Codec;
    use constellation_node::mesh::ConnectionRules;

    // Test allow_all
    let rules = ConnectionRules::allow_all();
    assert!(rules.allows_transport("tcp"));
    assert!(rules.allows_transport("anything"));
    assert!(rules.allows_codec(&Codec::Bincode));

    // Test only_transport
    let rules = ConnectionRules::only_transport("tls");
    assert!(rules.allows_transport("tls"));
    assert!(!rules.allows_transport("tcp"));
    assert!(rules.allows_codec(&Codec::Bincode)); // codecs unrestricted

    // Test only_codec
    let rules = ConnectionRules::only_codec(Codec::Bincode);
    assert!(rules.allows_transport("tcp")); // transports unrestricted
    assert!(rules.allows_codec(&Codec::Bincode));
    assert!(!rules.allows_codec(&Codec::Json));

    // Test only (transport + codec)
    let rules = ConnectionRules::only("tls", Codec::Bincode);
    assert!(rules.allows("tls", &Codec::Bincode));
    assert!(!rules.allows("tcp", &Codec::Bincode));
    assert!(!rules.allows("tls", &Codec::Json));
}

#[test]
fn test_constraint_allow_all() {
    let constraint = Constraint::allow_all();

    assert!(constraint.default.transports.is_empty());
    assert!(constraint.default.codecs.is_empty());
    assert!(constraint.per_network.is_empty());
}

#[test]
fn test_constraint_per_network() {
    use constellation_fabric::Codec;
    use constellation_node::mesh::ConnectionRules;

    let constraint = Constraint::allow_all()
        .with_network("external", ConnectionRules::only("tls", Codec::Bincode));

    // Internal (no specific rule) uses default (allow all)
    assert!(constraint.allows("internal", "tcp", &Codec::Json));

    // External has specific rules
    assert!(constraint.allows("external", "tls", &Codec::Bincode));
    assert!(!constraint.allows("external", "tcp", &Codec::Bincode));
    assert!(!constraint.allows("external", "tls", &Codec::Json));
}

#[test]
fn test_capabilities_basic() {
    let caps = Capabilities::basic();

    assert!(!caps.can_forward);
    assert!(!caps.can_translate);
    assert_eq!(caps.max_hops, None);
}

#[test]
fn test_capabilities_full() {
    let caps = Capabilities::full();

    assert!(caps.can_forward);
    assert!(caps.can_translate);
    assert_eq!(caps.max_hops, None);
}

#[test]
fn test_capabilities_builder() {
    let caps = Capabilities::builder()
        .can_forward(true)
        .can_translate(false)
        .max_hops(5)
        .build();

    assert!(caps.can_forward);
    assert!(!caps.can_translate);
    assert_eq!(caps.max_hops, Some(5));
}

#[test]
fn test_transponder_with_constraints() {
    use constellation_node::mesh::ConnectionRules;

    let global = Constraint::allow_all()
        .with_default(ConnectionRules::only_transport("tcp"));

    let route_specific = Constraint::allow_all()
        .with_default(ConnectionRules::only_transport("unix"));

    let data = TransponderData::builder()
        .node_id("constrained-node")
        .transport("tcp")
        .codec("bincode")
        .route("Service.method.v1")
        .global_constraints(global)
        .route_constraint("Service.method.v1", route_specific)
        .build();

    assert_eq!(data.global_constraints.default.transports, vec!["tcp"]);
    assert_eq!(
        data.route_constraints
            .get("Service.method.v1")
            .unwrap()
            .default
            .transports,
        vec!["unix"]
    );
}

#[test]
fn test_default_capabilities() {
    let caps = Capabilities::default();

    // Default should be basic (no forwarding)
    assert!(!caps.can_forward);
    assert!(!caps.can_translate);
}
