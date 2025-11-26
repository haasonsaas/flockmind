use crate::types::*;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HiveState {
    pub nodes: HashMap<NodeId, NodeStatus>,
    pub tasks: HashMap<TaskId, Task>,
    pub attachments: HashMap<AttachmentId, Attachment>,
    pub goals: HashMap<GoalId, Goal>,
    pub last_applied_index: u64,
}

impl HiveState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, command: &ClusterCommand) {
        match command {
            ClusterCommand::RegisterNode(status) => {
                self.nodes.insert(status.node_id.clone(), status.clone());
            }
            ClusterCommand::UpdateNodeHealth {
                node_id,
                health,
                metrics,
            } => {
                if let Some(node) = self.nodes.get_mut(node_id) {
                    node.health = health.clone();
                    node.cpu_usage = metrics.cpu_usage;
                    node.memory_usage = metrics.memory_usage;
                    node.disk_usage = metrics.disk_usage;
                    node.last_heartbeat = Utc::now();
                }
            }
            ClusterCommand::RemoveNode { node_id } => {
                self.nodes.remove(node_id);
            }
            ClusterCommand::PutTask(task) => {
                self.tasks.insert(task.id.clone(), task.clone());
            }
            ClusterCommand::UpdateTaskStatus {
                task_id,
                status,
                result,
            } => {
                if let Some(task) = self.tasks.get_mut(task_id) {
                    task.status = status.clone();
                    task.result = result.clone();
                    task.updated_at = Utc::now();
                }
            }
            ClusterCommand::PutAttachment(attachment) => {
                self.attachments
                    .insert(attachment.id.clone(), attachment.clone());
            }
            ClusterCommand::RemoveAttachment { attachment_id } => {
                self.attachments.remove(attachment_id);
            }
            ClusterCommand::PutGoal(goal) => {
                self.goals.insert(goal.id.clone(), goal.clone());
            }
            ClusterCommand::RemoveGoal { goal_id } => {
                self.goals.remove(goal_id);
            }
        }
    }

    pub fn to_cluster_view(&self, leader_id: Option<NodeId>, term: u64) -> ClusterView {
        ClusterView {
            nodes: self.nodes.values().cloned().collect(),
            tasks: self.tasks.values().cloned().collect(),
            attachments: self.attachments.values().cloned().collect(),
            goals: self.goals.values().cloned().collect(),
            leader_id,
            term,
        }
    }
}

#[derive(Clone)]
pub struct SharedState {
    inner: Arc<RwLock<HiveState>>,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HiveState::new())),
        }
    }

    pub fn apply(&self, command: &ClusterCommand) {
        let mut state = self.inner.write().unwrap();
        state.apply(command);
    }

    pub fn snapshot(&self) -> HiveState {
        self.inner.read().unwrap().clone()
    }

    pub fn to_cluster_view(&self, leader_id: Option<NodeId>, term: u64) -> ClusterView {
        self.inner.read().unwrap().to_cluster_view(leader_id, term)
    }

    pub fn set_last_applied(&self, index: u64) {
        self.inner.write().unwrap().last_applied_index = index;
    }

    pub fn last_applied(&self) -> u64 {
        self.inner.read().unwrap().last_applied_index
    }

    pub fn restore(&self, state: HiveState) {
        *self.inner.write().unwrap() = state;
    }
}

impl Default for SharedState {
    fn default() -> Self {
        Self::new()
    }
}
