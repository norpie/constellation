// Tests for mesh types and builders

use constellation_node::mesh::{AddressGroup, Capabilities, Constraint, TransponderData};

#[test]
fn test_transponder_data_builder() {
    let data = TransponderData::builder()
        .node_id("test-node-1")
        .transport("tcp")
        .transport("unix")
        .codec("bincode")
        .route("UserService.login.v1")
        .route("UserService.logout.v1")
        .address(AddressGroup::single("default", "tcp", "127.0.0.1:8080"))
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
fn test_address_group_builder() {
    let group = AddressGroup::builder()
        .zone("dc-east")
        .transport("tcp")
        .address("10.0.1.5:8080")
        .address("10.0.1.6:8080")
        .build();

    assert_eq!(group.zone, "dc-east");
    assert_eq!(group.transport, "tcp");
    assert_eq!(group.addresses.len(), 2);
}

#[test]
fn test_address_group_single() {
    let group = AddressGroup::single("public", "tcp", "203.0.113.5:8080");

    assert_eq!(group.zone, "public");
    assert_eq!(group.transport, "tcp");
    assert_eq!(group.addresses, vec!["203.0.113.5:8080"]);
}

#[test]
fn test_constraint_builder() {
    let constraint = Constraint::builder()
        .allow_transport("tcp")
        .allow_transport("unix")
        .deny_codec("protobuf")
        .allow_combination("tcp", "bincode")
        .build();

    assert_eq!(constraint.allow_transports, vec!["tcp", "unix"]);
    assert_eq!(constraint.deny_codecs, vec!["protobuf"]);
    assert_eq!(constraint.allow_combinations.len(), 1);
}

#[test]
fn test_constraint_allow_all() {
    let constraint = Constraint::allow_all();

    assert!(constraint.allow_transports.is_empty());
    assert!(constraint.deny_transports.is_empty());
    assert!(constraint.allow_codecs.is_empty());
    assert!(constraint.deny_codecs.is_empty());
}

#[test]
fn test_constraint_deny_all() {
    let constraint = Constraint::deny_all();

    // deny_all is represented by empty lists (no explicit allows)
    assert!(constraint.allow_transports.is_empty());
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
    let global = Constraint::builder()
        .allow_transport("tcp")
        .build();

    let route_specific = Constraint::builder()
        .allow_transport("unix")
        .build();

    let data = TransponderData::builder()
        .node_id("constrained-node")
        .transport("tcp")
        .codec("bincode")
        .route("Service.method.v1")
        .global_constraints(global)
        .route_constraint("Service.method.v1", route_specific)
        .build();

    assert_eq!(data.global_constraints.allow_transports, vec!["tcp"]);
    assert_eq!(
        data.route_constraints
            .get("Service.method.v1")
            .unwrap()
            .allow_transports,
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
