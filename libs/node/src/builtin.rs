// Built-in handlers for framework-internal RPC

use crate::config::{get_json_path, set_json_path, Config};
use crate::handler;
use crate::mesh::{AddressBook, AddressBookCommand, LeaveRequest, MeshResponse, TransponderData};
use crate::scheduler::Scheduler;
use crate::Data;
use constellation_fabric::Codec;
use constellation_raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    RaftNode, RequestVoteRequest, RequestVoteResponse,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Built-in handler for Raft RequestVote RPC
#[handler(route = "_raft.request_vote")]
async fn request_vote(
    req: RequestVoteRequest,
    raft: Data<RaftNode<AddressBook>>,
) -> Result<RequestVoteResponse, crate::error::Error> {
    raft.handle_request_vote(req)
        .await
        .map_err(crate::error::Error::from)
}

/// Built-in handler for Raft AppendEntries RPC
///
/// Handles both heartbeats and log replication from the leader.
/// On success, resets the election timeout to prevent unnecessary elections.
#[handler(route = "_raft.append_entries")]
async fn append_entries(
    req: AppendEntriesRequest,
    raft: Data<RaftNode<AddressBook>>,
    scheduler: Data<Scheduler>,
) -> Result<AppendEntriesResponse, crate::error::Error> {
    let resp = raft
        .handle_append_entries(req)
        .await
        .map_err(crate::error::Error::from)?;

    // Reset election timeout on successful heartbeat from leader
    // This prevents followers from starting unnecessary elections
    if resp.success {
        if let Some(handle) = scheduler.handle_by_name("election_timeout").await {
            handle.reset_now();
        }
    }

    Ok(resp)
}

/// Built-in handler for mesh join requests
///
/// Allows new nodes to join the cluster. If this node is the leader,
/// it submits a Join command to Raft. Otherwise, it returns information
/// about the current leader so the client can retry there.
#[handler(route = "_mesh.join")]
async fn mesh_join(
    req: TransponderData,
    raft: Data<RaftNode<AddressBook>>,
) -> Result<MeshResponse, crate::error::Error> {
    println!("[_mesh.join] Received join request from node: {}", req.node_id);

    // Check if we're the leader
    let is_leader = raft.is_leader().await;
    println!("[_mesh.join] Am I leader? {}", is_leader);

    if !is_leader {
        // Get leader info from AddressBook if we know who it is
        let leader_id = raft.current_leader().await;
        println!("[_mesh.join] Current leader: {:?}", leader_id);
        let leader_data = match leader_id {
            Some(id) => raft
                .with_state_machine(|ab| ab.get_node(&id).cloned())
                .await,
            None => None,
        };
        println!("[_mesh.join] Returning NotLeader response");
        return Ok(MeshResponse::NotLeader { leader: leader_data });
    }

    // Serialize the Join command
    let command = AddressBookCommand::Join(req.clone());
    let bytes = Codec::Bincode
        .encode(&command)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    // Submit to Raft
    println!("[_mesh.join] Submitting join command to Raft...");
    raft.submit_command(bytes)
        .await
        .map_err(crate::error::Error::from)?;

    println!("[_mesh.join] Join successful for node: {}", req.node_id);
    Ok(MeshResponse::Success)
}

/// Built-in handler for mesh leave requests
///
/// Allows nodes to leave the cluster gracefully. If this node is the leader,
/// it submits a Leave command to Raft. Otherwise, it returns information
/// about the current leader so the client can retry there.
#[handler(route = "_mesh.leave")]
async fn mesh_leave(
    req: LeaveRequest,
    raft: Data<RaftNode<AddressBook>>,
) -> Result<MeshResponse, crate::error::Error> {
    // Check if we're the leader
    if !raft.is_leader().await {
        // Get leader info from AddressBook if we know who it is
        let leader_id = raft.current_leader().await;
        let leader_data = match leader_id {
            Some(id) => raft
                .with_state_machine(|ab| ab.get_node(&id).cloned())
                .await,
            None => None,
        };
        return Ok(MeshResponse::NotLeader { leader: leader_data });
    }

    // Serialize the Leave command
    let command = AddressBookCommand::Leave(req.node_id);
    let bytes = Codec::Bincode
        .encode(&command)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    // Submit to Raft
    raft.submit_command(bytes)
        .await
        .map_err(crate::error::Error::from)?;

    Ok(MeshResponse::Success)
}

/// Built-in handler for Raft InstallSnapshot RPC
///
/// Handles snapshot installation from the leader when a follower is too far
/// behind to catch up via normal log replication.
#[handler(route = "_raft.install_snapshot")]
async fn install_snapshot(
    req: InstallSnapshotRequest,
    raft: Data<RaftNode<AddressBook>>,
    scheduler: Data<Scheduler>,
) -> Result<InstallSnapshotResponse, crate::error::Error> {
    let resp = raft
        .handle_install_snapshot(req)
        .await
        .map_err(crate::error::Error::from)?;

    // Reset election timeout - valid leader contact
    if let Some(handle) = scheduler.handle_by_name("election_timeout").await {
        handle.reset_now();
    }

    Ok(resp)
}

// ============================================================================
// Config management handlers
// ============================================================================

/// Request for getting config values
#[derive(Debug, Deserialize)]
pub struct ConfigGetRequest {
    /// Optional dot-separated path. If empty, returns full config.
    #[serde(default)]
    pub path: Option<String>,
}

/// Request for setting a config value
#[derive(Debug, Deserialize)]
pub struct ConfigSetRequest {
    /// Dot-separated path to the config field
    pub path: String,
    /// New value to set
    pub value: serde_json::Value,
}

/// Response for config operations
#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Empty request for handlers that don't need input
#[derive(Debug, Deserialize)]
pub struct EmptyRequest {}

/// Get config value(s)
///
/// If path is provided, returns the value at that path.
/// Otherwise, returns the full config.
#[handler(route = "_config.get")]
async fn config_get(
    req: ConfigGetRequest,
    config: Data<RwLock<Config>>,
) -> Result<serde_json::Value, crate::error::Error> {
    let cfg = config.read().await;
    let json = serde_json::to_value(&*cfg)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    match req.path {
        Some(path) if !path.is_empty() => {
            let value = get_json_path(&json, &path)
                .map_err(|e| crate::error::Error::Custom(e.to_string()))?;
            Ok(value.clone())
        }
        _ => Ok(json),
    }
}

/// Set a config value
///
/// The value is validated by deserializing the entire config after modification.
/// If validation fails, the config is not changed.
#[handler(route = "_config.set")]
async fn config_set(
    req: ConfigSetRequest,
    config: Data<RwLock<Config>>,
) -> Result<ConfigResponse, crate::error::Error> {
    let mut cfg = config.write().await;

    // Serialize current config to JSON
    let mut json = serde_json::to_value(&*cfg)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    // Modify at path
    set_json_path(&mut json, &req.path, req.value)
        .map_err(|e| crate::error::Error::Custom(e.to_string()))?;

    // Deserialize back - this validates the change
    *cfg = serde_json::from_value(json)
        .map_err(|e| crate::error::Error::Custom(format!("Invalid config: {}", e)))?;

    Ok(ConfigResponse {
        success: true,
        error: None,
    })
}

/// Get JSON Schema for the config
///
/// If path is provided, returns the schema for that field.
/// Otherwise, returns the full schema.
#[handler(route = "_config.schema")]
async fn config_schema(
    req: ConfigGetRequest,
) -> Result<serde_json::Value, crate::error::Error> {
    let schema = schemars::schema_for!(Config);
    let json = serde_json::to_value(&schema)
        .map_err(|e| crate::error::Error::Serialization(e.to_string()))?;

    match req.path {
        Some(path) if !path.is_empty() => {
            // Convert path to schema path: "rpc.timeout" -> "properties.rpc.properties.timeout"
            let schema_path = path
                .split('.')
                .map(|s| format!("properties.{}", s))
                .collect::<Vec<_>>()
                .join(".");

            let value = get_json_path(&json, &schema_path)
                .map_err(|e| crate::error::Error::Custom(e.to_string()))?;
            Ok(value.clone())
        }
        _ => Ok(json),
    }
}

/// Reset config to defaults
#[handler(route = "_config.reset")]
async fn config_reset(
    _req: EmptyRequest,
    config: Data<RwLock<Config>>,
) -> Result<ConfigResponse, crate::error::Error> {
    let mut cfg = config.write().await;
    *cfg = Config::default();

    Ok(ConfigResponse {
        success: true,
        error: None,
    })
}
