use crate::replicator::storage::TypeConfig;
use crate::replicator::RaftReplicator;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use openraft::raft::{
    AppendEntriesRequest, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest,
};
use std::sync::Arc;

pub fn create_raft_router(replicator: Arc<RaftReplicator>) -> Router {
    Router::new()
        .route("/raft/vote", post(handle_vote))
        .route("/raft/append_entries", post(handle_append_entries))
        .route("/raft/install_snapshot", post(handle_install_snapshot))
        .with_state(replicator)
}

async fn handle_vote(
    State(replicator): State<Arc<RaftReplicator>>,
    Json(req): Json<VoteRequest<u64>>,
) -> impl IntoResponse {
    match replicator.raft().vote(req).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn handle_append_entries(
    State(replicator): State<Arc<RaftReplicator>>,
    Json(req): Json<AppendEntriesRequest<TypeConfig>>,
) -> impl IntoResponse {
    match replicator.raft().append_entries(req).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn handle_install_snapshot(
    State(replicator): State<Arc<RaftReplicator>>,
    Json(req): Json<InstallSnapshotRequest<TypeConfig>>,
) -> impl IntoResponse {
    let resp: Result<InstallSnapshotResponse<u64>, _> = replicator.raft().install_snapshot(req).await;
    match resp {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
