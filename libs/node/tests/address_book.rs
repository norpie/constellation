//! Tests for address book

use constellation_node::mesh::{
    AddressBook, AddressBookCommand, AddressBookResponse, AdvertisedAddress, Capabilities,
    TransponderData,
};
use constellation_raft::StateMachine;

#[tokio::test]
async fn test_address_book_join() {
    let mut book = AddressBook::new();

    let data = TransponderData::builder()
        .node_id("node-1")
        .transport("tcp")
        .codec("bincode")
        .route("Service.method.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:8080"))
        .capabilities(Capabilities::basic())
        .build();

    let response = book.apply(AddressBookCommand::Join(data)).await.unwrap();
    assert!(matches!(response, AddressBookResponse::Success));

    // Verify node was added
    assert!(book.get_node("node-1").is_some());
    assert_eq!(book.get_nodes_for_route("Service.method.v1").unwrap().len(), 1);
}

#[tokio::test]
async fn test_address_book_join_upsert() {
    let mut book = AddressBook::new();

    let data1 = TransponderData::builder()
        .node_id("node-1")
        .transport("tcp")
        .codec("bincode")
        .route("Service.method.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:8080"))
        .capabilities(Capabilities::basic())
        .build();

    book.apply(AddressBookCommand::Join(data1)).await.unwrap();

    // Join again with updated data (upsert behavior)
    let data2 = TransponderData::builder()
        .node_id("node-1")
        .transport("tcp")
        .codec("bincode")
        .route("Service.method.v2") // Different route
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9090")) // Different address
        .capabilities(Capabilities::basic())
        .build();

    let response = book.apply(AddressBookCommand::Join(data2)).await.unwrap();
    assert!(matches!(response, AddressBookResponse::Success));

    // Old route should be gone, new route should exist
    assert!(book.get_nodes_for_route("Service.method.v1").is_none());
    assert_eq!(book.get_nodes_for_route("Service.method.v2").unwrap().len(), 1);

    // Address should be updated
    let node = book.get_node("node-1").unwrap();
    assert_eq!(node.addresses[0].address, "127.0.0.1:9090");
}

#[tokio::test]
async fn test_address_book_leave() {
    let mut book = AddressBook::new();

    let data = TransponderData::builder()
        .node_id("node-1")
        .transport("tcp")
        .codec("bincode")
        .route("Service.method.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:8080"))
        .capabilities(Capabilities::basic())
        .build();

    book.apply(AddressBookCommand::Join(data)).await.unwrap();

    let response = book.apply(AddressBookCommand::Leave("node-1".to_string())).await.unwrap();
    assert!(matches!(response, AddressBookResponse::Success));

    // Verify node was removed
    assert!(book.get_node("node-1").is_none());
    assert!(book.get_nodes_for_route("Service.method.v1").is_none());
}

#[tokio::test]
async fn test_address_book_update() {
    let mut book = AddressBook::new();

    let data1 = TransponderData::builder()
        .node_id("node-1")
        .transport("tcp")
        .codec("bincode")
        .route("Service.method.v1")
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:8080"))
        .capabilities(Capabilities::basic())
        .build();

    book.apply(AddressBookCommand::Join(data1)).await.unwrap();

    let data2 = TransponderData::builder()
        .node_id("node-1")
        .transport("tcp")
        .codec("bincode")
        .route("Service.method.v2") // Different route
        .address(AdvertisedAddress::new("default", "tcp", "127.0.0.1:9090"))
        .capabilities(Capabilities::basic())
        .build();

    let response = book
        .apply(AddressBookCommand::Update("node-1".to_string(), data2))
        .await
        .unwrap();

    assert!(matches!(response, AddressBookResponse::Success));

    // Old route should be gone
    assert!(book.get_nodes_for_route("Service.method.v1").is_none());

    // New route should exist
    assert_eq!(book.get_nodes_for_route("Service.method.v2").unwrap().len(), 1);
}
