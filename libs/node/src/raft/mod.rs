//! Raft consensus module
//!
//! This module contains Raft-related handlers and background tasks.

mod handlers;
mod tasks;

pub use tasks::schedule_raft_tasks;
