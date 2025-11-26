use crate::types::*;
use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    pub allow_restart_services: bool,
    pub allow_docker: bool,
    pub allowed_sync_paths: Vec<String>,
    pub blocked_sync_paths: Vec<String>,
    pub require_approval_for_destructive: bool,
    pub max_concurrent_tasks_per_node: usize,
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            allow_restart_services: false,
            allow_docker: false,
            allowed_sync_paths: vec!["/home".to_string(), "/data".to_string()],
            blocked_sync_paths: vec![
                "/etc".to_string(),
                "/var".to_string(),
                "/usr".to_string(),
                "/bin".to_string(),
                "/sbin".to_string(),
                "/root".to_string(),
            ],
            require_approval_for_destructive: true,
            max_concurrent_tasks_per_node: 5,
        }
    }
}

pub struct ActionValidator {
    policy: ExecutionPolicy,
}

impl ActionValidator {
    pub fn new(policy: ExecutionPolicy) -> Self {
        Self { policy }
    }

    pub fn validate(&self, action: &BrainAction, cluster: &ClusterView) -> Result<()> {
        match action {
            BrainAction::ScheduleTask {
                task, target_node, ..
            } => {
                self.validate_node_exists(target_node, cluster)?;
                self.validate_task_policy(task)?;
                self.validate_task_limit(target_node, cluster)?;
            }
            BrainAction::RebalanceTask { task_id, to_node } => {
                self.validate_node_exists(to_node, cluster)?;
                self.validate_task_exists(task_id, cluster)?;
            }
            BrainAction::CancelTask { task_id } => {
                self.validate_task_exists(task_id, cluster)?;
            }
            BrainAction::MarkNodeDegraded { node_id, .. } => {
                self.validate_node_exists(node_id, cluster)?;
            }
            BrainAction::CreateAttachment { node_id, kind, .. } => {
                self.validate_node_exists(node_id, cluster)?;
                self.validate_attachment_kind(kind)?;
            }
            BrainAction::RemoveAttachment { attachment_id } => {
                self.validate_attachment_exists(attachment_id, cluster)?;
            }
            BrainAction::UpdateGoalProgress { goal_id, .. } => {
                self.validate_goal_exists(goal_id, cluster)?;
            }
            BrainAction::RequestHumanApproval { .. } | BrainAction::NoOp { .. } => {}
        }

        Ok(())
    }

    fn validate_node_exists(&self, node_id: &str, cluster: &ClusterView) -> Result<()> {
        if cluster.node_by_id(node_id).is_none() {
            return Err(anyhow!("Node '{}' not found in cluster", node_id));
        }
        Ok(())
    }

    fn validate_task_exists(&self, task_id: &str, cluster: &ClusterView) -> Result<()> {
        if !cluster.tasks.iter().any(|t| t.id == task_id) {
            return Err(anyhow!("Task '{}' not found", task_id));
        }
        Ok(())
    }

    fn validate_goal_exists(&self, goal_id: &str, cluster: &ClusterView) -> Result<()> {
        if !cluster.goals.iter().any(|g| g.id == goal_id) {
            return Err(anyhow!("Goal '{}' not found", goal_id));
        }
        Ok(())
    }

    fn validate_attachment_exists(&self, attachment_id: &str, cluster: &ClusterView) -> Result<()> {
        if !cluster.attachments.iter().any(|a| a.id == attachment_id) {
            return Err(anyhow!("Attachment '{}' not found", attachment_id));
        }
        Ok(())
    }

    fn validate_task_policy(&self, task: &TaskPayload) -> Result<()> {
        match task {
            TaskPayload::Echo { .. } | TaskPayload::CheckService { .. } => Ok(()),

            TaskPayload::RestartService { .. } => {
                if !self.policy.allow_restart_services {
                    return Err(anyhow!(
                        "Policy: service restart not allowed"
                    ));
                }
                Ok(())
            }

            TaskPayload::DockerRun { .. } => {
                if !self.policy.allow_docker {
                    return Err(anyhow!("Policy: Docker execution not allowed"));
                }
                Ok(())
            }

            TaskPayload::SyncDirectory { src, dst } => {
                self.validate_path_allowed(src)?;
                self.validate_path_allowed(dst)?;
                Ok(())
            }

            TaskPayload::RunCommand { command, .. } => {
                Err(anyhow!(
                    "Policy: arbitrary command execution not allowed: {}",
                    command
                ))
            }

            TaskPayload::Custom { tool_id, .. } => {
                Err(anyhow!(
                    "Policy: custom tool '{}' not pre-approved",
                    tool_id
                ))
            }
        }
    }

    fn validate_path_allowed(&self, path: &str) -> Result<()> {
        for blocked in &self.policy.blocked_sync_paths {
            if path.starts_with(blocked) {
                return Err(anyhow!("Policy: path '{}' is blocked", path));
            }
        }

        let allowed = self.policy.allowed_sync_paths.is_empty()
            || self
                .policy
                .allowed_sync_paths
                .iter()
                .any(|p| path.starts_with(p));

        if !allowed {
            return Err(anyhow!(
                "Policy: path '{}' not in allowed paths",
                path
            ));
        }

        Ok(())
    }

    fn validate_attachment_kind(&self, kind: &AttachmentKind) -> Result<()> {
        match kind {
            AttachmentKind::Directory { path } | AttachmentKind::File { path } => {
                self.validate_path_allowed(path)
            }
            AttachmentKind::DockerContainer { .. } => {
                if !self.policy.allow_docker {
                    return Err(anyhow!(
                        "Policy: Docker attachments not allowed"
                    ));
                }
                Ok(())
            }
            AttachmentKind::Service { .. }
            | AttachmentKind::Webhook { .. }
            | AttachmentKind::Custom { .. } => Ok(()),
        }
    }

    fn validate_task_limit(&self, node_id: &str, cluster: &ClusterView) -> Result<()> {
        let active_tasks = cluster
            .tasks
            .iter()
            .filter(|t| {
                t.target_node == node_id
                    && matches!(t.status, TaskStatus::Pending | TaskStatus::Running)
            })
            .count();

        if active_tasks >= self.policy.max_concurrent_tasks_per_node {
            return Err(anyhow!(
                "Policy: node '{}' has {} active tasks (max: {})",
                node_id,
                active_tasks,
                self.policy.max_concurrent_tasks_per_node
            ));
        }

        Ok(())
    }
}
