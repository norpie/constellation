//! Lexer for the query language.
//!
//! Tokenizes a query string into a stream of tokens.

// TODO: Implement lexer
//
// Token types needed:
// - Keywords: logs, metrics, spans, last, from, to, by
// - Aggregations: rate, avg, sum, min, max, count, p50, p90, p95, p99
// - Operators: =, !=, =~, !~, >, >=, <, <=, |, |=, |~
// - Delimiters: {, }, (, ), ,
// - Literals: strings, numbers, durations, timestamps, identifiers
// - Levels: debug, info, warn, error
