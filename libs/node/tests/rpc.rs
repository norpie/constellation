//! Tests for RPC frame packing/parsing

use constellation_node::rpc::{pack_frame, parse_frame, RpcHeader};
use constellation_telemetry::{SpanId, TraceId};
use uuid::Uuid;

#[test]
fn test_pack_parse_frame_roundtrip() {
    let header = RpcHeader {
        request_id: Uuid::new_v4(),
        route: "TestService.method.v1".to_string(),
        trace_id: None,
        parent_span_id: None,
    };
    let payload = b"test payload data";

    // Pack frame
    let frame = pack_frame(&header, payload).expect("pack_frame should succeed");

    // Verify frame structure
    assert!(frame.len() >= 4 + payload.len());
    let header_len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    assert_eq!(frame.len(), 4 + header_len + payload.len());

    // Parse frame
    let (parsed_header, parsed_payload) =
        parse_frame(&frame).expect("parse_frame should succeed");

    // Verify round-trip
    assert_eq!(parsed_header.request_id, header.request_id);
    assert_eq!(parsed_header.route, header.route);
    assert_eq!(parsed_payload, payload);
}

#[test]
fn test_parse_frame_too_short() {
    let frame = vec![0, 0, 0]; // Only 3 bytes
    let result = parse_frame(&frame);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Frame too short"));
}

#[test]
fn test_parse_frame_incomplete() {
    // Create a valid header but truncate the frame
    let header = RpcHeader {
        request_id: Uuid::new_v4(),
        route: "Test.method.v1".to_string(),
        trace_id: None,
        parent_span_id: None,
    };
    let payload = b"payload";

    let full_frame = pack_frame(&header, payload).unwrap();

    // Truncate to incomplete frame
    let incomplete = &full_frame[..full_frame.len() / 2];

    let result = parse_frame(incomplete);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Incomplete frame"));
}

#[test]
fn test_frame_with_empty_payload() {
    let header = RpcHeader {
        request_id: Uuid::new_v4(),
        route: "Empty.test.v1".to_string(),
        trace_id: None,
        parent_span_id: None,
    };
    let payload = b"";

    let frame = pack_frame(&header, payload).unwrap();
    let (parsed_header, parsed_payload) = parse_frame(&frame).unwrap();

    assert_eq!(parsed_header.request_id, header.request_id);
    assert_eq!(parsed_header.route, header.route);
    assert_eq!(parsed_payload, payload);
}

#[test]
fn test_frame_with_large_payload() {
    let header = RpcHeader {
        request_id: Uuid::new_v4(),
        route: "Large.payload.v1".to_string(),
        trace_id: None,
        parent_span_id: None,
    };
    let payload = vec![0xAB; 10000]; // 10KB payload

    let frame = pack_frame(&header, &payload).unwrap();
    let (parsed_header, parsed_payload) = parse_frame(&frame).unwrap();

    assert_eq!(parsed_header.request_id, header.request_id);
    assert_eq!(parsed_header.route, header.route);
    assert_eq!(parsed_payload, &payload[..]);
}

#[test]
fn test_frame_with_trace_context() {
    let trace_id = TraceId::new();
    let span_id = SpanId::new();

    let header = RpcHeader {
        request_id: Uuid::new_v4(),
        route: "Traced.method.v1".to_string(),
        trace_id: Some(trace_id.clone()),
        parent_span_id: Some(span_id.clone()),
    };
    let payload = b"traced payload";

    let frame = pack_frame(&header, payload).unwrap();
    let (parsed_header, parsed_payload) = parse_frame(&frame).unwrap();

    assert_eq!(parsed_header.request_id, header.request_id);
    assert_eq!(parsed_header.route, header.route);
    assert_eq!(parsed_payload, payload);

    // Verify trace context is preserved
    assert!(parsed_header.trace_id.is_some());
    assert!(parsed_header.parent_span_id.is_some());
    assert_eq!(
        parsed_header.trace_id.as_ref().map(|t| t.as_str()),
        Some(trace_id.as_str())
    );
    assert_eq!(
        parsed_header.parent_span_id.as_ref().map(|s| s.as_str()),
        Some(span_id.as_str())
    );
}
