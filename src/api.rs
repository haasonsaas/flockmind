use crate::daemon::HiveDaemon;
use crate::replicator::Replicator;
use crate::types::*;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub fn create_router(daemon: Arc<HiveDaemon>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/status", get(get_status))
        .route("/cluster", get(get_cluster_view))
        .route("/tasks", get(list_tasks))
        .route("/tasks", post(submit_task))
        .route("/goals", get(list_goals))
        .route("/goals", post(add_goal))
        .route("/attachments", get(list_attachments))
        .with_state(daemon)
}

async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

#[derive(Serialize)]
struct StatusResponse {
    node_id: String,
    is_leader: bool,
    leader_id: Option<String>,
    cluster_size: usize,
}

async fn get_status(State(daemon): State<Arc<HiveDaemon>>) -> impl IntoResponse {
    let view = daemon.replicator().snapshot();
    let is_leader = daemon.replicator().is_leader();
    let leader_id = daemon.replicator().leader_id();

    Json(StatusResponse {
        node_id: daemon.node_id().to_string(),
        is_leader,
        leader_id,
        cluster_size: view.nodes.len(),
    })
}

async fn get_cluster_view(State(daemon): State<Arc<HiveDaemon>>) -> impl IntoResponse {
    let view = daemon.replicator().snapshot();
    Json(view)
}

async fn list_tasks(State(daemon): State<Arc<HiveDaemon>>) -> impl IntoResponse {
    let view = daemon.replicator().snapshot();
    Json(view.tasks)
}

#[derive(Deserialize)]
struct SubmitTaskRequest {
    target_node: String,
    payload: TaskPayload,
    priority: Option<u8>,
}

async fn submit_task(
    State(daemon): State<Arc<HiveDaemon>>,
    Json(req): Json<SubmitTaskRequest>,
) -> impl IntoResponse {
    let task = Task {
        id: uuid::Uuid::new_v4().to_string(),
        target_node: req.target_node,
        payload: req.payload,
        status: TaskStatus::Pending,
        priority: req.priority.unwrap_or(5),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        result: None,
    };

    match daemon
        .replicator()
        .apply(ClusterCommand::PutTask(task.clone()))
        .await
    {
        Ok(_) => (StatusCode::CREATED, Json(task)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn list_goals(State(daemon): State<Arc<HiveDaemon>>) -> impl IntoResponse {
    let view = daemon.replicator().snapshot();
    Json(view.goals)
}

#[derive(Deserialize)]
struct AddGoalRequest {
    description: String,
    constraints: Option<Vec<String>>,
    priority: Option<u8>,
}

async fn add_goal(
    State(daemon): State<Arc<HiveDaemon>>,
    Json(req): Json<AddGoalRequest>,
) -> impl IntoResponse {
    let goal = Goal {
        id: uuid::Uuid::new_v4().to_string(),
        description: req.description,
        constraints: req.constraints.unwrap_or_default(),
        priority: req.priority.unwrap_or(5),
        active: true,
        created_at: chrono::Utc::now(),
    };

    match daemon
        .replicator()
        .apply(ClusterCommand::PutGoal(goal.clone()))
        .await
    {
        Ok(_) => (StatusCode::CREATED, Json(goal)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn list_attachments(State(daemon): State<Arc<HiveDaemon>>) -> impl IntoResponse {
    let attachments = daemon.attachments().list();
    Json(attachments)
}
