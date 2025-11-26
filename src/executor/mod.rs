mod runner;
mod validator;

pub use runner::*;
pub use validator::*;

use crate::replicator::Replicator;
use crate::types::*;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute(&self, action: BrainAction) -> Result<()>;
    async fn run_task(&self, task: &Task) -> Result<serde_json::Value>;
}

pub struct HiveExecutor<R: Replicator> {
    node_id: String,
    replicator: Arc<R>,
    validator: ActionValidator,
    runner: TaskRunner,
}

impl<R: Replicator + 'static> HiveExecutor<R> {
    pub fn new(node_id: String, replicator: Arc<R>, policy: ExecutionPolicy) -> Self {
        Self {
            node_id,
            replicator,
            validator: ActionValidator::new(policy),
            runner: TaskRunner::new(),
        }
    }
}

#[async_trait]
impl<R: Replicator + 'static> Executor for HiveExecutor<R> {
    async fn execute(&self, action: BrainAction) -> Result<()> {
        self.validator.validate(&action, &self.replicator.snapshot())?;

        match action {
            BrainAction::ScheduleTask {
                task,
                target_node,
                priority,
            } => {
                let task = Task {
                    id: uuid::Uuid::new_v4().to_string(),
                    target_node,
                    payload: task,
                    status: TaskStatus::Pending,
                    priority,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    result: None,
                };
                self.replicator
                    .apply(ClusterCommand::PutTask(task))
                    .await?;
            }
            BrainAction::CancelTask { task_id } => {
                self.replicator
                    .apply(ClusterCommand::UpdateTaskStatus {
                        task_id,
                        status: TaskStatus::Cancelled,
                        result: None,
                    })
                    .await?;
            }
            BrainAction::RebalanceTask { task_id, to_node } => {
                let view = self.replicator.snapshot();
                if let Some(task) = view.tasks.iter().find(|t| t.id == task_id) {
                    let mut new_task = task.clone();
                    new_task.target_node = to_node;
                    new_task.status = TaskStatus::Pending;
                    new_task.updated_at = chrono::Utc::now();
                    self.replicator
                        .apply(ClusterCommand::PutTask(new_task))
                        .await?;
                }
            }
            BrainAction::MarkNodeDegraded { node_id, reason } => {
                self.replicator
                    .apply(ClusterCommand::UpdateNodeHealth {
                        node_id,
                        health: NodeHealth::Degraded { reason },
                        metrics: NodeMetrics::default(),
                    })
                    .await?;
            }
            BrainAction::CreateAttachment {
                node_id,
                kind,
                capabilities,
            } => {
                let attachment = Attachment {
                    id: uuid::Uuid::new_v4().to_string(),
                    node_id,
                    kind,
                    capabilities,
                    metadata: std::collections::HashMap::new(),
                    created_at: chrono::Utc::now(),
                };
                self.replicator
                    .apply(ClusterCommand::PutAttachment(attachment))
                    .await?;
            }
            BrainAction::RemoveAttachment { attachment_id } => {
                self.replicator
                    .apply(ClusterCommand::RemoveAttachment { attachment_id })
                    .await?;
            }
            BrainAction::UpdateGoalProgress { goal_id, .. } => {
                tracing::info!(
                    "Goal {} progress updated (not persisted in v0)",
                    goal_id
                );
            }
            BrainAction::RequestHumanApproval {
                action_description,
                severity,
            } => {
                tracing::warn!(
                    "HUMAN APPROVAL REQUIRED [{}]: {}",
                    severity,
                    action_description
                );
            }
            BrainAction::NoOp { reason } => {
                tracing::debug!("NoOp: {}", reason);
            }
        }

        Ok(())
    }

    async fn run_task(&self, task: &Task) -> Result<serde_json::Value> {
        if task.target_node != self.node_id {
            anyhow::bail!(
                "Task {} targeted at {}, but this is node {}",
                task.id,
                task.target_node,
                self.node_id
            );
        }

        self.replicator
            .apply(ClusterCommand::UpdateTaskStatus {
                task_id: task.id.clone(),
                status: TaskStatus::Running,
                result: None,
            })
            .await?;

        let result = self.runner.run(&task.payload).await;

        let (status, result_value) = match result {
            Ok(value) => (TaskStatus::Completed, Some(value)),
            Err(e) => (
                TaskStatus::Failed {
                    error: e.to_string(),
                },
                None,
            ),
        };

        self.replicator
            .apply(ClusterCommand::UpdateTaskStatus {
                task_id: task.id.clone(),
                status,
                result: result_value.clone(),
            })
            .await?;

        result_value.ok_or_else(|| anyhow::anyhow!("Task failed"))
    }
}
