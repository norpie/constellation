//! Query Language for Telemetry
//!
//! A pipeline-based DSL inspired by PromQL/LogQL for querying telemetry data.
//!
//! # Syntax Overview
//!
//! ```text
//! selector | filter | filter | aggregation | timerange
//! ```
//!
//! # Examples
//!
//! ```text
//! logs{service="auth"}                           # All logs from auth service
//! logs{service="auth"} | level=error | last 1h   # Error logs from last hour
//! metrics{name="latency"} | p99 | by(endpoint)   # P99 latency by endpoint
//! spans{service="api"} | duration > 100ms        # Slow spans
//! *{trace_id="abc123"}                           # Everything for a trace
//! ```
//!
//! # Grammar
//!
//! ```text
//! query       = selector pipeline?
//! selector    = type "{" labels? "}"
//! type        = "logs" | "metrics" | "spans" | "*"
//! labels      = label ("," label)*
//! label       = key op value
//! op          = "=" | "!=" | "=~" | "!~"
//!
//! pipeline    = ("|" stage)*
//! stage       = content_filter | field_filter | aggregation | timerange
//!
//! content_filter = "|=" string | "|~" string
//! field_filter   = key compare_op value
//! compare_op     = "=" | "!=" | ">" | ">=" | "<" | "<="
//!
//! aggregation = "rate" "(" duration ")"
//!             | "avg" | "sum" | "min" | "max" | "count"
//!             | "p50" | "p90" | "p95" | "p99"
//!             | "by" "(" key ("," key)* ")"
//!
//! timerange   = "last" duration
//!             | "from" timestamp "to" timestamp
//!
//! duration    = number ("s" | "m" | "h" | "d")
//! ```

pub mod ast;

mod lexer;
mod parser;

pub use ast::*;
pub use parser::{parse, ParseError};
