use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type NodeId = String;
pub type TaskId = String;
pub type AttachmentId = String;
pub type GoalId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub node_id: NodeId,
    pub public_key: Vec<u8>,
    pub hostname: String,
    pub enrolled_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeHealth {
    Healthy,
    Degraded { reason: String },
    Unreachable,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node_id: NodeId,
    pub hostname: String,
    pub tags: Vec<String>,
    pub health: NodeHealth,
    pub last_heartbeat: DateTime<Utc>,
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub disk_usage: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttachmentKind {
    Directory { path: String },
    File { path: String },
    DockerContainer { container_id: String },
    Service { name: String, unit: Option<String> },
    Webhook { url: String },
    Custom { type_name: String, config: serde_json::Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: AttachmentId,
    pub node_id: NodeId,
    pub kind: AttachmentKind,
    pub capabilities: Vec<String>,
    pub metadata: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Scheduled,
    Running,
    Completed,
    Failed { error: String },
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskPayload {
    Echo { message: String },
    SyncDirectory { src: String, dst: String },
    RunCommand { command: String, args: Vec<String> },
    CheckService { service_name: String },
    RestartService { service_name: String },
    DockerRun { image: String, args: Vec<String> },
    Custom { tool_id: String, args: serde_json::Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub target_node: NodeId,
    pub payload: TaskPayload,
    pub status: TaskStatus,
    pub priority: u8,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: GoalId,
    pub description: String,
    pub constraints: Vec<String>,
    pub priority: u8,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterView {
    pub nodes: Vec<NodeStatus>,
    pub tasks: Vec<Task>,
    pub attachments: Vec<Attachment>,
    pub goals: Vec<Goal>,
    pub leader_id: Option<NodeId>,
    pub term: u64,
}

impl ClusterView {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            tasks: Vec::new(),
            attachments: Vec::new(),
            goals: Vec::new(),
            leader_id: None,
            term: 0,
        }
    }

    pub fn node_by_id(&self, id: &str) -> Option<&NodeStatus> {
        self.nodes.iter().find(|n| n.node_id == id)
    }

    pub fn healthy_nodes(&self) -> Vec<&NodeStatus> {
        self.nodes
            .iter()
            .filter(|n| n.health == NodeHealth::Healthy)
            .collect()
    }

    pub fn nodes_with_tag(&self, tag: &str) -> Vec<&NodeStatus> {
        self.nodes
            .iter()
            .filter(|n| n.tags.iter().any(|t| t == tag))
            .collect()
    }

    pub fn pending_tasks(&self) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Pending)
            .collect()
    }

    pub fn tasks_for_node(&self, node_id: &str) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|t| t.target_node == node_id)
            .collect()
    }
}

impl Default for ClusterView {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BrainAction {
    ScheduleTask {
        task: TaskPayload,
        target_node: NodeId,
        priority: u8,
    },
    RebalanceTask {
        task_id: TaskId,
        to_node: NodeId,
    },
    CancelTask {
        task_id: TaskId,
    },
    UpdateGoalProgress {
        goal_id: GoalId,
        progress_percent: u8,
        notes: Option<String>,
    },
    CreateAttachment {
        node_id: NodeId,
        kind: AttachmentKind,
        capabilities: Vec<String>,
    },
    RemoveAttachment {
        attachment_id: AttachmentId,
    },
    MarkNodeDegraded {
        node_id: NodeId,
        reason: String,
    },
    RequestHumanApproval {
        action_description: String,
        severity: String,
    },
    NoOp {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClusterCommand {
    RegisterNode(NodeStatus),
    UpdateNodeHealth {
        node_id: NodeId,
        health: NodeHealth,
        metrics: NodeMetrics,
    },
    RemoveNode {
        node_id: NodeId,
    },
    PutTask(Task),
    UpdateTaskStatus {
        task_id: TaskId,
        status: TaskStatus,
        result: Option<serde_json::Value>,
    },
    PutAttachment(Attachment),
    RemoveAttachment {
        attachment_id: AttachmentId,
    },
    PutGoal(Goal),
    RemoveGoal {
        goal_id: GoalId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeMetrics {
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub disk_usage: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentToken {
    pub token: String,
    pub cluster_id: String,
    pub expires_at: DateTime<Utc>,
    pub allowed_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentResponse {
    pub node_id: NodeId,
    pub cluster_id: String,
    pub peers: Vec<PeerInfo>,
    pub certificate: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub node_id: NodeId,
    pub addr: String,
    pub is_voter: bool,
}
