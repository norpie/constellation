//! Tests for label matcher operators (=, !=, =~, !~).

use constellation_telemetry_service::query::*;

#[test]
fn equal_operator() {
    let q = parse(r#"logs{service="auth"}"#).unwrap();
    assert_eq!(q.selector.labels[0].matcher, MatchOp::Equal("auth".into()));
}

#[test]
fn not_equal_operator() {
    let q = parse(r#"logs{service!="internal"}"#).unwrap();
    assert_eq!(
        q.selector.labels[0].matcher,
        MatchOp::NotEqual("internal".into())
    );
}

#[test]
fn regex_match_operator() {
    let q = parse(r#"logs{service=~"auth|api"}"#).unwrap();
    assert_eq!(
        q.selector.labels[0].matcher,
        MatchOp::Regex("auth|api".into())
    );
}

#[test]
fn regex_not_match_operator() {
    let q = parse(r#"logs{service!~"test.*"}"#).unwrap();
    assert_eq!(
        q.selector.labels[0].matcher,
        MatchOp::NotRegex("test.*".into())
    );
}

#[test]
fn mixed_operators() {
    let q = parse(r#"logs{service="auth", env!="dev", region=~"us-.*"}"#).unwrap();
    assert_eq!(q.selector.labels.len(), 3);
    assert_eq!(q.selector.labels[0].matcher, MatchOp::Equal("auth".into()));
    assert_eq!(
        q.selector.labels[1].matcher,
        MatchOp::NotEqual("dev".into())
    );
    assert_eq!(
        q.selector.labels[2].matcher,
        MatchOp::Regex("us-.*".into())
    );
}

#[test]
fn escaped_quotes_in_value() {
    let q = parse(r#"logs{message="said \"hello\""}"#).unwrap();
    assert_eq!(
        q.selector.labels[0].matcher,
        MatchOp::Equal(r#"said "hello""#.into())
    );
}
