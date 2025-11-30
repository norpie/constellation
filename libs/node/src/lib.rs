pub mod codec;
pub mod error;
pub mod handler;
pub mod mesh;
pub mod raft_tasks;
pub mod router;
pub mod rpc;
pub mod scheduler;

mod builtin;
mod node;

// Re-export the derive macro
pub use constellation_node_derive::handler;

// Re-export commonly used types
pub use codec::{BincodeFactory, CodecFactory, RawCodecFactory, TypedCodec};
pub use error::{Error, Result};
pub use node::{Data, Node, NodeBuilder};
pub use rpc::{
    ErrorCategory, ErrorResponder, HandlerError, Responder, RpcClient, RpcRequest, RpcResponse,
    ResponseResult,
};
pub use router::{ResolvedTarget, Router, RoutingError};
pub use scheduler::{
    OverlapPolicy, Schedule, Scheduler, TaskContext, TaskHandle, TaskId, TaskInfo, TaskStatus,
};
