//! Parser for the query language.
//!
//! Parses a query string into an AST.

use std::time::Duration;

use super::ast::*;
use super::lexer::{LexError, Lexer, Token};
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

    #[error("Lexer error: {0:?}")]
    LexError(LexError),
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        match e {
            LexError::UnclosedString => ParseError::UnclosedString,
            LexError::InvalidDurationUnit(u) => ParseError::InvalidDuration(u),
            other => ParseError::LexError(other),
        }
    }
}

/// Parser state holding the lexer and current position.
struct Parser<'a> {
    lexer: Lexer<'a>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            lexer: Lexer::new(input),
        }
    }

    /// Peek at the next token.
    fn peek(&mut self) -> Result<Option<&Token>, ParseError> {
        Ok(self.lexer.peek()?)
    }

    /// Consume and return the next token.
    fn next(&mut self) -> Result<Option<Token>, ParseError> {
        Ok(self.lexer.next()?)
    }

    /// Expect and consume a specific token.
    fn expect(&mut self, expected: Token) -> Result<(), ParseError> {
        match self.next()? {
            Some(tok) if tok == expected => Ok(()),
            Some(tok) => Err(ParseError::Expected {
                expected: format!("{:?}", expected),
                found: format!("{:?}", tok),
            }),
            None => Err(ParseError::Expected {
                expected: format!("{:?}", expected),
                found: "end of input".into(),
            }),
        }
    }

    /// Check if the next token matches, without consuming.
    fn check(&mut self, token: &Token) -> Result<bool, ParseError> {
        Ok(self.peek()?.map(|t| t == token).unwrap_or(false))
    }

    /// Parse a complete query.
    fn parse_query(&mut self) -> Result<Query, ParseError> {
        let selector = self.parse_selector()?;
        let pipeline = self.parse_pipeline()?;

        Ok(Query { selector, pipeline })
    }

    /// Parse the selector: `type{labels}` or `*{labels}`
    fn parse_selector(&mut self) -> Result<Selector, ParseError> {
        let entry_type = match self.peek()? {
            Some(Token::Logs) => {
                self.next()?;
                Some(EntryType::Log)
            }
            Some(Token::Metrics) => {
                self.next()?;
                Some(EntryType::Metric)
            }
            Some(Token::Spans) => {
                self.next()?;
                Some(EntryType::Span)
            }
            Some(Token::Star) => {
                self.next()?;
                None
            }
            Some(tok) => {
                return Err(ParseError::InvalidEntryType(format!("{:?}", tok)));
            }
            None => return Err(ParseError::UnexpectedEof),
        };

        self.expect(Token::LBrace)?;
        let labels = self.parse_labels()?;

        // Check for closing brace
        match self.next()? {
            Some(Token::RBrace) => {}
            Some(_) => return Err(ParseError::UnclosedBrace),
            None => return Err(ParseError::UnclosedBrace),
        }

        Ok(Selector { entry_type, labels })
    }

    /// Parse label matchers: `key=value, key!=value, ...`
    fn parse_labels(&mut self) -> Result<Vec<LabelMatcher>, ParseError> {
        let mut labels = Vec::new();

        // Check if we have any labels
        if self.check(&Token::RBrace)? {
            return Ok(labels);
        }

        // Parse first label
        labels.push(self.parse_label()?);

        // Parse additional labels separated by commas
        while self.check(&Token::Comma)? {
            self.next()?; // consume comma
            labels.push(self.parse_label()?);
        }

        Ok(labels)
    }

    /// Parse a single label matcher: `key op "value"`
    fn parse_label(&mut self) -> Result<LabelMatcher, ParseError> {
        // Get the key (identifier)
        let key = match self.next()? {
            Some(Token::Ident(s)) => s,
            Some(tok) => {
                return Err(ParseError::Expected {
                    expected: "identifier".into(),
                    found: format!("{:?}", tok),
                })
            }
            None => return Err(ParseError::UnexpectedEof),
        };

        // Get the operator
        let matcher = match self.next()? {
            Some(Token::Eq) => {
                let value = self.parse_string_value()?;
                MatchOp::Equal(value)
            }
            Some(Token::Ne) => {
                let value = self.parse_string_value()?;
                MatchOp::NotEqual(value)
            }
            Some(Token::RegexEq) => {
                let value = self.parse_string_value()?;
                MatchOp::Regex(value)
            }
            Some(Token::RegexNe) => {
                let value = self.parse_string_value()?;
                MatchOp::NotRegex(value)
            }
            Some(tok) => return Err(ParseError::InvalidOperator(format!("{:?}", tok))),
            None => return Err(ParseError::UnexpectedEof),
        };

        Ok(LabelMatcher { key, matcher })
    }

    /// Parse a string value (quoted string).
    fn parse_string_value(&mut self) -> Result<String, ParseError> {
        match self.next()? {
            Some(Token::String(s)) => Ok(s),
            Some(tok) => Err(ParseError::Expected {
                expected: "string".into(),
                found: format!("{:?}", tok),
            }),
            None => Err(ParseError::UnexpectedEof),
        }
    }

    /// Parse the pipeline: `| stage | stage | ...`
    fn parse_pipeline(&mut self) -> Result<Vec<PipelineStage>, ParseError> {
        let mut stages = Vec::new();

        while self.check(&Token::Pipe)? || self.check(&Token::PipeContains)? || self.check(&Token::PipeRegex)? {
            let stage = self.parse_pipeline_stage()?;
            stages.push(stage);
        }

        Ok(stages)
    }

    /// Parse a single pipeline stage.
    fn parse_pipeline_stage(&mut self) -> Result<PipelineStage, ParseError> {
        match self.next()? {
            // Content filter: |= "text" or |~ "regex"
            Some(Token::PipeContains) => {
                let value = self.parse_string_value()?;
                Ok(PipelineStage::ContentFilter(ContentFilter::Contains(value)))
            }
            Some(Token::PipeRegex) => {
                let value = self.parse_string_value()?;
                Ok(PipelineStage::ContentFilter(ContentFilter::Regex(value)))
            }

            // Everything else after a pipe
            Some(Token::Pipe) => self.parse_stage_after_pipe(),

            Some(tok) => Err(ParseError::UnexpectedToken(format!("{:?}", tok))),
            None => Err(ParseError::UnexpectedEof),
        }
    }

    /// Parse a stage after a `|` token.
    fn parse_stage_after_pipe(&mut self) -> Result<PipelineStage, ParseError> {
        match self.peek()? {
            // Time range: last duration
            Some(Token::Last) => {
                self.next()?;
                let duration = self.parse_duration()?;
                Ok(PipelineStage::TimeRange(TimeRange::Last(duration)))
            }

            // Time range: from timestamp to timestamp
            Some(Token::From) => {
                self.next()?;
                let from = self.parse_timestamp()?;
                self.expect(Token::To)?;
                let to = self.parse_timestamp()?;
                Ok(PipelineStage::TimeRange(TimeRange::Absolute { from, to }))
            }

            // Group by: by(field, field, ...)
            Some(Token::By) => {
                self.next()?;
                self.expect(Token::LParen)?;
                let fields = self.parse_ident_list()?;
                if fields.is_empty() {
                    return Err(ParseError::Expected {
                        expected: "at least one field".into(),
                        found: "empty list".into(),
                    });
                }
                self.expect(Token::RParen)?;
                Ok(PipelineStage::GroupBy(fields))
            }

            // Aggregations
            Some(Token::Rate) => {
                self.next()?;
                self.expect(Token::LParen)?;
                let duration = self.parse_duration()?;
                self.expect(Token::RParen)?;
                Ok(PipelineStage::Aggregation(Aggregation::Rate(duration)))
            }
            Some(Token::Avg) => {
                self.next()?;
                Ok(PipelineStage::Aggregation(Aggregation::Avg))
            }
            Some(Token::Sum) => {
                self.next()?;
                Ok(PipelineStage::Aggregation(Aggregation::Sum))
            }
            Some(Token::Min) => {
                self.next()?;
                Ok(PipelineStage::Aggregation(Aggregation::Min))
            }
            Some(Token::Max) => {
                self.next()?;
                Ok(PipelineStage::Aggregation(Aggregation::Max))
            }
            Some(Token::Count) => {
                self.next()?;
                Ok(PipelineStage::Aggregation(Aggregation::Count))
            }
            Some(Token::P50) => {
                self.next()?;
                Ok(PipelineStage::Aggregation(Aggregation::P50))
            }
            Some(Token::P90) => {
                self.next()?;
                Ok(PipelineStage::Aggregation(Aggregation::P90))
            }
            Some(Token::P95) => {
                self.next()?;
                Ok(PipelineStage::Aggregation(Aggregation::P95))
            }
            Some(Token::P99) => {
                self.next()?;
                Ok(PipelineStage::Aggregation(Aggregation::P99))
            }

            // Field filter: field op value
            Some(Token::Ident(_)) => self.parse_field_filter(),

            // After pipe but nothing recognized
            Some(tok) => Err(ParseError::InvalidAggregation(format!("{:?}", tok))),
            None => Err(ParseError::UnexpectedEof),
        }
    }

    /// Parse a field filter: `field op value`
    fn parse_field_filter(&mut self) -> Result<PipelineStage, ParseError> {
        let field = match self.next()? {
            Some(Token::Ident(s)) => s,
            _ => unreachable!("called after peeking Ident"),
        };

        // Get the comparison operator
        let op = match self.next()? {
            Some(Token::Eq) => CompareOp::Eq,
            Some(Token::Ne) => CompareOp::Ne,
            Some(Token::Gt) => CompareOp::Gt,
            Some(Token::Ge) => CompareOp::Ge,
            Some(Token::Lt) => CompareOp::Lt,
            Some(Token::Le) => CompareOp::Le,
            Some(tok) => return Err(ParseError::InvalidOperator(format!("{:?}", tok))),
            None => return Err(ParseError::UnexpectedEof),
        };

        // Get the value
        let value = self.parse_field_value()?;

        Ok(PipelineStage::FieldFilter(FieldFilter { field, op, value }))
    }

    /// Parse a field value (string, number, duration, or log level).
    fn parse_field_value(&mut self) -> Result<FieldValue, ParseError> {
        match self.next()? {
            Some(Token::String(s)) => Ok(FieldValue::String(s)),
            Some(Token::Number(n)) => Ok(FieldValue::Number(n)),
            Some(Token::Duration { value, unit }) => {
                let millis = unit.to_millis(value);
                Ok(FieldValue::Duration(Duration::from_millis(millis)))
            }
            Some(Token::Debug) => Ok(FieldValue::Level(LogLevel::Debug)),
            Some(Token::Info) => Ok(FieldValue::Level(LogLevel::Info)),
            Some(Token::Warn) => Ok(FieldValue::Level(LogLevel::Warn)),
            Some(Token::Error) => Ok(FieldValue::Level(LogLevel::Error)),
            Some(tok) => Err(ParseError::Expected {
                expected: "value".into(),
                found: format!("{:?}", tok),
            }),
            None => Err(ParseError::UnexpectedEof),
        }
    }

    /// Parse a duration value.
    fn parse_duration(&mut self) -> Result<Duration, ParseError> {
        match self.next()? {
            Some(Token::Duration { value, unit }) => {
                let millis = unit.to_millis(value);
                Ok(Duration::from_millis(millis))
            }
            Some(tok) => Err(ParseError::InvalidDuration(format!("{:?}", tok))),
            None => Err(ParseError::UnexpectedEof),
        }
    }

    /// Parse a timestamp (for absolute time ranges).
    /// Supports ISO 8601 format or Unix timestamp in milliseconds.
    fn parse_timestamp(&mut self) -> Result<u64, ParseError> {
        match self.next()? {
            Some(Token::Number(n)) => Ok(n as u64),
            Some(Token::String(s)) => {
                // Try to parse ISO 8601 timestamp
                self.parse_iso_timestamp(&s)
            }
            Some(Token::Timestamp(s)) => {
                // ISO 8601 timestamp from lexer
                self.parse_iso_timestamp(&s)
            }
            Some(tok) => Err(ParseError::InvalidTimestamp(format!("{:?}", tok))),
            None => Err(ParseError::UnexpectedEof),
        }
    }

    /// Parse an ISO 8601 timestamp string to microseconds since epoch.
    fn parse_iso_timestamp(&self, s: &str) -> Result<u64, ParseError> {
        // Simple ISO 8601 parser: YYYY-MM-DDTHH:MM:SSZ
        // For a full implementation, we'd use chrono, but let's keep it simple
        // Format: 2024-01-15T10:30:00Z

        if s.len() < 19 {
            return Err(ParseError::InvalidTimestamp(s.to_string()));
        }

        let parts: Vec<&str> = s.split(|c| c == '-' || c == 'T' || c == ':' || c == 'Z').collect();
        if parts.len() < 6 {
            return Err(ParseError::InvalidTimestamp(s.to_string()));
        }

        let year: i32 = parts[0].parse().map_err(|_| ParseError::InvalidTimestamp(s.to_string()))?;
        let month: u32 = parts[1].parse().map_err(|_| ParseError::InvalidTimestamp(s.to_string()))?;
        let day: u32 = parts[2].parse().map_err(|_| ParseError::InvalidTimestamp(s.to_string()))?;
        let hour: u32 = parts[3].parse().map_err(|_| ParseError::InvalidTimestamp(s.to_string()))?;
        let min: u32 = parts[4].parse().map_err(|_| ParseError::InvalidTimestamp(s.to_string()))?;
        let sec: u32 = parts[5].parse().map_err(|_| ParseError::InvalidTimestamp(s.to_string()))?;

        // Convert to Unix timestamp (simplified, doesn't handle leap years perfectly)
        // Days from year 1970
        let days_since_epoch = {
            let mut days = 0i64;
            for y in 1970..year {
                days += if is_leap_year(y) { 366 } else { 365 };
            }
            for m in 1..month {
                days += days_in_month(year, m) as i64;
            }
            days += (day - 1) as i64;
            days
        };

        let secs = days_since_epoch * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64;

        // Convert to microseconds
        Ok((secs as u64) * 1_000_000)
    }

    /// Parse a comma-separated list of identifiers.
    fn parse_ident_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut idents = Vec::new();

        // Check if the list is empty
        if self.check(&Token::RParen)? {
            return Ok(idents);
        }

        // Parse first identifier
        match self.next()? {
            Some(Token::Ident(s)) => idents.push(s),
            Some(tok) => {
                return Err(ParseError::Expected {
                    expected: "identifier".into(),
                    found: format!("{:?}", tok),
                })
            }
            None => return Err(ParseError::UnexpectedEof),
        }

        // Parse additional identifiers
        while self.check(&Token::Comma)? {
            self.next()?; // consume comma
            match self.next()? {
                Some(Token::Ident(s)) => idents.push(s),
                Some(tok) => {
                    return Err(ParseError::Expected {
                        expected: "identifier".into(),
                        found: format!("{:?}", tok),
                    })
                }
                None => return Err(ParseError::UnexpectedEof),
            }
        }

        Ok(idents)
    }
}

/// Check if a year is a leap year.
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Get the number of days in a month.
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 => 31,
        2 => if is_leap_year(year) { 29 } else { 28 },
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => 30,
    }
}

/// Parse a query string into an AST.
pub fn parse(input: &str) -> Result<Query, ParseError> {
    if input.trim().is_empty() {
        return Err(ParseError::EmptyQuery);
    }

    let mut parser = Parser::new(input);
    parser.parse_query()
}
