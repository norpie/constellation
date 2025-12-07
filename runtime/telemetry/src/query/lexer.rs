//! Lexer for the query language.
//!
//! Tokenizes a query string into a stream of tokens.

use std::iter::Peekable;
use std::str::Chars;

/// Token types produced by the lexer.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Entry types
    Logs,
    Metrics,
    Spans,
    Star, // *

    // Keywords
    Last,
    From,
    To,
    By,

    // Aggregations
    Rate,
    Avg,
    Sum,
    Min,
    Max,
    Count,
    P50,
    P90,
    P95,
    P99,

    // Log levels (used as values)
    Debug,
    Info,
    Warn,
    Error,

    // Operators
    Eq,       // =
    Ne,       // !=
    RegexEq,  // =~
    RegexNe,  // !~
    Gt,       // >
    Ge,       // >=
    Lt,       // <
    Le,       // <=

    // Pipeline operators
    Pipe,            // |
    PipeContains,    // |=
    PipeRegex,       // |~

    // Delimiters
    LBrace,   // {
    RBrace,   // }
    LParen,   // (
    RParen,   // )
    Comma,    // ,

    // Literals
    String(String),     // "quoted string"
    Number(f64),        // 123, 45.67
    Ident(String),      // field names, label keys

    // Duration units (number + unit parsed together)
    Duration { value: u64, unit: DurationUnit },

    // ISO 8601 timestamp (2024-01-01T00:00:00Z)
    Timestamp(String),
}

/// Duration unit for parsing durations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationUnit {
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl DurationUnit {
    pub fn to_millis(self, value: u64) -> u64 {
        match self {
            DurationUnit::Milliseconds => value,
            DurationUnit::Seconds => value * 1000,
            DurationUnit::Minutes => value * 60 * 1000,
            DurationUnit::Hours => value * 60 * 60 * 1000,
            DurationUnit::Days => value * 24 * 60 * 60 * 1000,
        }
    }
}

/// Error type for lexer failures.
#[derive(Debug, Clone, PartialEq)]
pub enum LexError {
    UnexpectedChar(char),
    UnclosedString,
    InvalidNumber(String),
    InvalidDurationUnit(String),
}

/// Lexer for the query language.
pub struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
    peeked: Option<Token>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input: input.chars().peekable(),
            peeked: None,
        }
    }

    /// Peek at the next token without consuming it.
    pub fn peek(&mut self) -> Result<Option<&Token>, LexError> {
        if self.peeked.is_none() {
            self.peeked = self.next_token()?;
        }
        Ok(self.peeked.as_ref())
    }

    /// Consume and return the next token.
    pub fn next(&mut self) -> Result<Option<Token>, LexError> {
        if let Some(token) = self.peeked.take() {
            return Ok(Some(token));
        }
        self.next_token()
    }

    /// Skip whitespace characters.
    fn skip_whitespace(&mut self) {
        while let Some(&c) = self.input.peek() {
            if c.is_whitespace() {
                self.input.next();
            } else {
                break;
            }
        }
    }

    /// Read the next token from input.
    fn next_token(&mut self) -> Result<Option<Token>, LexError> {
        self.skip_whitespace();

        let c = match self.input.next() {
            Some(c) => c,
            None => return Ok(None),
        };

        let token = match c {
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '(' => Token::LParen,
            ')' => Token::RParen,
            ',' => Token::Comma,
            '*' => Token::Star,

            '|' => {
                match self.input.peek() {
                    Some('=') => {
                        self.input.next();
                        Token::PipeContains
                    }
                    Some('~') => {
                        self.input.next();
                        Token::PipeRegex
                    }
                    _ => Token::Pipe,
                }
            }

            '=' => {
                match self.input.peek() {
                    Some('~') => {
                        self.input.next();
                        Token::RegexEq
                    }
                    _ => Token::Eq,
                }
            }

            '!' => {
                match self.input.peek() {
                    Some('=') => {
                        self.input.next();
                        Token::Ne
                    }
                    Some('~') => {
                        self.input.next();
                        Token::RegexNe
                    }
                    _ => return Err(LexError::UnexpectedChar('!')),
                }
            }

            '>' => {
                match self.input.peek() {
                    Some('=') => {
                        self.input.next();
                        Token::Ge
                    }
                    _ => Token::Gt,
                }
            }

            '<' => {
                match self.input.peek() {
                    Some('=') => {
                        self.input.next();
                        Token::Le
                    }
                    _ => Token::Lt,
                }
            }

            '"' => self.read_string()?,

            c if c.is_ascii_digit() => self.read_number(c)?,

            c if c.is_alphabetic() || c == '_' => self.read_ident(c)?,

            _ => return Err(LexError::UnexpectedChar(c)),
        };

        Ok(Some(token))
    }

    /// Read a quoted string literal.
    fn read_string(&mut self) -> Result<Token, LexError> {
        let mut s = String::new();

        loop {
            match self.input.next() {
                Some('"') => break,
                Some('\\') => {
                    // Handle escape sequences
                    match self.input.next() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('r') => s.push('\r'),
                        Some('\\') => s.push('\\'),
                        Some('"') => s.push('"'),
                        Some(c) => {
                            s.push('\\');
                            s.push(c);
                        }
                        None => return Err(LexError::UnclosedString),
                    }
                }
                Some(c) => s.push(c),
                None => return Err(LexError::UnclosedString),
            }
        }

        Ok(Token::String(s))
    }

    /// Read a number, possibly followed by a duration unit, or an ISO 8601 timestamp.
    fn read_number(&mut self, first: char) -> Result<Token, LexError> {
        let mut s = String::new();
        s.push(first);

        // Read integer part
        while let Some(&c) = self.input.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.input.next();
            } else {
                break;
            }
        }

        // Check if this might be an ISO 8601 timestamp (YYYY-MM-DD...)
        // If we have exactly 4 digits followed by '-', treat as timestamp
        if s.len() == 4 && self.input.peek() == Some(&'-') {
            return self.read_timestamp(s);
        }

        // Check for decimal point
        if self.input.peek() == Some(&'.') {
            s.push('.');
            self.input.next();

            while let Some(&c) = self.input.peek() {
                if c.is_ascii_digit() {
                    s.push(c);
                    self.input.next();
                } else {
                    break;
                }
            }
        }

        // Check for duration unit
        if let Some(&c) = self.input.peek() {
            if c.is_alphabetic() {
                let mut unit = String::new();
                while let Some(&c) = self.input.peek() {
                    if c.is_alphabetic() {
                        unit.push(c);
                        self.input.next();
                    } else {
                        break;
                    }
                }

                let duration_unit = match unit.as_str() {
                    "ms" => DurationUnit::Milliseconds,
                    "s" => DurationUnit::Seconds,
                    "m" => DurationUnit::Minutes,
                    "h" => DurationUnit::Hours,
                    "d" => DurationUnit::Days,
                    _ => return Err(LexError::InvalidDurationUnit(unit)),
                };

                let value: u64 = s.parse().map_err(|_| LexError::InvalidNumber(s))?;
                return Ok(Token::Duration { value, unit: duration_unit });
            }
        }

        // Plain number
        let n: f64 = s.parse().map_err(|_| LexError::InvalidNumber(s))?;
        Ok(Token::Number(n))
    }

    /// Read an ISO 8601 timestamp: YYYY-MM-DDTHH:MM:SSZ
    fn read_timestamp(&mut self, year: String) -> Result<Token, LexError> {
        let mut s = year;

        // Read the rest of the timestamp: -MM-DDTHH:MM:SSZ
        // Valid chars: digits, -, T, :, Z
        while let Some(&c) = self.input.peek() {
            if c.is_ascii_digit() || c == '-' || c == 'T' || c == ':' || c == 'Z' {
                s.push(c);
                self.input.next();
                // Stop after Z
                if c == 'Z' {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(Token::Timestamp(s))
    }

    /// Read an identifier or keyword.
    fn read_ident(&mut self, first: char) -> Result<Token, LexError> {
        let mut s = String::new();
        s.push(first);

        while let Some(&c) = self.input.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.input.next();
            } else {
                break;
            }
        }

        // Check for keywords
        let token = match s.as_str() {
            // Entry types
            "logs" => Token::Logs,
            "metrics" => Token::Metrics,
            "spans" => Token::Spans,

            // Keywords
            "last" => Token::Last,
            "from" => Token::From,
            "to" => Token::To,
            "by" => Token::By,

            // Aggregations
            "rate" => Token::Rate,
            "avg" => Token::Avg,
            "sum" => Token::Sum,
            "min" => Token::Min,
            "max" => Token::Max,
            "count" => Token::Count,
            "p50" => Token::P50,
            "p90" => Token::P90,
            "p95" => Token::P95,
            "p99" => Token::P99,

            // Log levels
            "debug" => Token::Debug,
            "info" => Token::Info,
            "warn" => Token::Warn,
            "error" => Token::Error,

            // Everything else is an identifier
            _ => Token::Ident(s),
        };

        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        while let Ok(Some(token)) = lexer.next() {
            tokens.push(token);
        }
        tokens
    }

    #[test]
    fn simple_selector() {
        let tokens = lex("logs{}");
        assert_eq!(tokens, vec![Token::Logs, Token::LBrace, Token::RBrace]);
    }

    #[test]
    fn selector_with_label() {
        let tokens = lex(r#"logs{service="auth"}"#);
        assert_eq!(tokens, vec![
            Token::Logs,
            Token::LBrace,
            Token::Ident("service".into()),
            Token::Eq,
            Token::String("auth".into()),
            Token::RBrace,
        ]);
    }

    #[test]
    fn duration_units() {
        assert_eq!(lex("5ms"), vec![Token::Duration { value: 5, unit: DurationUnit::Milliseconds }]);
        assert_eq!(lex("10s"), vec![Token::Duration { value: 10, unit: DurationUnit::Seconds }]);
        assert_eq!(lex("30m"), vec![Token::Duration { value: 30, unit: DurationUnit::Minutes }]);
        assert_eq!(lex("1h"), vec![Token::Duration { value: 1, unit: DurationUnit::Hours }]);
        assert_eq!(lex("7d"), vec![Token::Duration { value: 7, unit: DurationUnit::Days }]);
    }

    #[test]
    fn operators() {
        assert_eq!(lex("="), vec![Token::Eq]);
        assert_eq!(lex("!="), vec![Token::Ne]);
        assert_eq!(lex("=~"), vec![Token::RegexEq]);
        assert_eq!(lex("!~"), vec![Token::RegexNe]);
        assert_eq!(lex(">"), vec![Token::Gt]);
        assert_eq!(lex(">="), vec![Token::Ge]);
        assert_eq!(lex("<"), vec![Token::Lt]);
        assert_eq!(lex("<="), vec![Token::Le]);
    }

    #[test]
    fn pipe_operators() {
        assert_eq!(lex("|"), vec![Token::Pipe]);
        assert_eq!(lex("|="), vec![Token::PipeContains]);
        assert_eq!(lex("|~"), vec![Token::PipeRegex]);
    }

    #[test]
    fn full_query() {
        let tokens = lex(r#"logs{service="auth"} | level=error | last 1h"#);
        assert_eq!(tokens, vec![
            Token::Logs,
            Token::LBrace,
            Token::Ident("service".into()),
            Token::Eq,
            Token::String("auth".into()),
            Token::RBrace,
            Token::Pipe,
            Token::Ident("level".into()),
            Token::Eq,
            Token::Error,
            Token::Pipe,
            Token::Last,
            Token::Duration { value: 1, unit: DurationUnit::Hours },
        ]);
    }
}
