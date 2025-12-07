//! Parser for the query language.
//!
//! Parses a query string into an AST.

use super::ast::Query;
use thiserror::Error;

/// Error type for parse failures.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Unexpected end of input")]
    UnexpectedEof,

    #[error("Unexpected token: {0}")]
    UnexpectedToken(String),

    #[error("Invalid entry type: {0}")]
    InvalidEntryType(String),

    #[error("Invalid operator: {0}")]
    InvalidOperator(String),

    #[error("Invalid duration: {0}")]
    InvalidDuration(String),

    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(String),

    #[error("Invalid aggregation: {0}")]
    InvalidAggregation(String),

    #[error("Invalid log level: {0}")]
    InvalidLogLevel(String),

    #[error("Unclosed string literal")]
    UnclosedString,

    #[error("Unclosed brace")]
    UnclosedBrace,

    #[error("Expected {expected}, found {found}")]
    Expected { expected: String, found: String },

    #[error("Empty query")]
    EmptyQuery,
}

/// Parse a query string into an AST.
pub fn parse(input: &str) -> Result<Query, ParseError> {
    if input.trim().is_empty() {
        return Err(ParseError::EmptyQuery);
    }

    // TODO: Implement parser
    todo!("Parser not yet implemented")
}
