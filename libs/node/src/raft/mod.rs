//! Raft consensus module
//!
//! This module contains Raft-related handlers and background tasks.

use constellation_fabric::Codec;

mod handlers;
mod tasks;

pub use tasks::schedule_raft_tasks;

/// Codec used for serializing commands in the Raft log.
///
/// This is separate from the RPC wire codec. Commands are encoded with this
/// codec before being submitted to Raft, and decoded when applied from the log.
pub const RAFT_LOG_CODEC: Codec = Codec::Bincode;
