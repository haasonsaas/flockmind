use crate::brain::{Brain, LlmClient, LlmConfig};
use crate::types::*;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

const SYSTEM_PROMPT: &str = r#"You are the planning brain for a distributed hive system called FlockMind.
Your job is to analyze the current cluster state, goals, and attachments, then propose actions to achieve the goals.

IMPORTANT CONSTRAINTS:
1. You can ONLY emit actions from the allowed action types
2. You must NEVER propose actions that would affect hosts not in the cluster
3. You must NEVER propose shell commands or arbitrary code execution
4. Prioritize stability and safety over speed
5. When unsure, emit a RequestHumanApproval action

Available action types:
- ScheduleTask: Schedule a task on a specific node
- RebalanceTask: Move a task from one node to another
- CancelTask: Cancel a running/pending task
- UpdateGoalProgress: Report progress on a goal
- CreateAttachment: Register a new attachment on a node
- RemoveAttachment: Remove an attachment
- MarkNodeDegraded: Flag a node as having issues
- RequestHumanApproval: Ask for human approval before proceeding
- NoOp: Do nothing (explain why)

Respond with a JSON object containing:
{
  "reasoning": "Brief explanation of your analysis",
  "actions": [
    // array of action objects
  ]
}

Each action must be one of these formats:
{ "type": "ScheduleTask", "task": { "type": "Echo", "message": "..." }, "target_node": "node_id", "priority": 5 }
{ "type": "ScheduleTask", "task": { "type": "SyncDirectory", "src": "/path", "dst": "/path" }, "target_node": "node_id", "priority": 5 }
{ "type": "ScheduleTask", "task": { "type": "CheckService", "service_name": "..." }, "target_node": "node_id", "priority": 5 }
{ "type": "RebalanceTask", "task_id": "...", "to_node": "..." }
{ "type": "CancelTask", "task_id": "..." }
{ "type": "UpdateGoalProgress", "goal_id": "...", "progress_percent": 50, "notes": "..." }
{ "type": "MarkNodeDegraded", "node_id": "...", "reason": "..." }
{ "type": "RequestHumanApproval", "action_description": "...", "severity": "low|medium|high" }
{ "type": "NoOp", "reason": "..." }
"#;

#[derive(Debug, Serialize)]
struct PlannerInput {
    goals: Vec<GoalSummary>,
    cluster: ClusterSummary,
    attachments: Vec<AttachmentSummary>,
}

#[derive(Debug, Serialize)]
struct GoalSummary {
    id: String,
    description: String,
    priority: u8,
}

#[derive(Debug, Serialize)]
struct ClusterSummary {
    node_count: usize,
    healthy_nodes: Vec<NodeSummary>,
    degraded_nodes: Vec<NodeSummary>,
    pending_tasks: usize,
    running_tasks: usize,
    leader_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct NodeSummary {
    node_id: String,
    hostname: String,
    tags: Vec<String>,
    cpu_usage: f32,
    memory_usage: f32,
    disk_usage: f32,
    task_count: usize,
}

#[derive(Debug, Serialize)]
struct AttachmentSummary {
    id: String,
    node_id: String,
    kind: String,
    capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PlannerOutput {
    reasoning: String,
    actions: Vec<RawAction>,
}

#[derive(Debug, Deserialize)]
struct RawAction {
    #[serde(rename = "type")]
    action_type: String,
    #[serde(flatten)]
    fields: serde_json::Value,
}

pub struct LlmPlanner {
    client: LlmClient,
}

impl LlmPlanner {
    pub fn new(config: LlmConfig) -> Result<Self> {
        let client = LlmClient::new(config)?;
        Ok(Self { client })
    }

    fn build_input(
        goals: &[Goal],
        cluster: &ClusterView,
        attachments: &[Attachment],
    ) -> PlannerInput {
        let goal_summaries: Vec<_> = goals
            .iter()
            .filter(|g| g.active)
            .map(|g| GoalSummary {
                id: g.id.clone(),
                description: g.description.clone(),
                priority: g.priority,
            })
            .collect();

        let mut healthy_nodes = Vec::new();
        let mut degraded_nodes = Vec::new();

        for node in &cluster.nodes {
            let task_count = cluster
                .tasks
                .iter()
                .filter(|t| {
                    t.target_node == node.node_id
                        && matches!(t.status, TaskStatus::Running | TaskStatus::Pending)
                })
                .count();

            let summary = NodeSummary {
                node_id: node.node_id.clone(),
                hostname: node.hostname.clone(),
                tags: node.tags.clone(),
                cpu_usage: node.cpu_usage,
                memory_usage: node.memory_usage,
                disk_usage: node.disk_usage,
                task_count,
            };

            match &node.health {
                NodeHealth::Healthy => healthy_nodes.push(summary),
                _ => degraded_nodes.push(summary),
            }
        }

        let pending_tasks = cluster
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Pending)
            .count();
        let running_tasks = cluster
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Running)
            .count();

        let cluster_summary = ClusterSummary {
            node_count: cluster.nodes.len(),
            healthy_nodes,
            degraded_nodes,
            pending_tasks,
            running_tasks,
            leader_id: cluster.leader_id.clone(),
        };

        let attachment_summaries: Vec<_> = attachments
            .iter()
            .map(|a| AttachmentSummary {
                id: a.id.clone(),
                node_id: a.node_id.clone(),
                kind: format!("{:?}", a.kind),
                capabilities: a.capabilities.clone(),
            })
            .collect();

        PlannerInput {
            goals: goal_summaries,
            cluster: cluster_summary,
            attachments: attachment_summaries,
        }
    }

    fn parse_action(raw: &RawAction) -> Result<BrainAction> {
        match raw.action_type.as_str() {
            "ScheduleTask" => {
                let task_obj = raw.fields.get("task").ok_or_else(|| anyhow!("Missing task"))?;
                let task_type = task_obj
                    .get("type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing task type"))?;

                let task = match task_type {
                    "Echo" => TaskPayload::Echo {
                        message: task_obj
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    },
                    "SyncDirectory" => TaskPayload::SyncDirectory {
                        src: task_obj
                            .get("src")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        dst: task_obj
                            .get("dst")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    },
                    "CheckService" => TaskPayload::CheckService {
                        service_name: task_obj
                            .get("service_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    },
                    "RestartService" => TaskPayload::RestartService {
                        service_name: task_obj
                            .get("service_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    },
                    _ => {
                        return Err(anyhow!("Unknown task type: {}", task_type));
                    }
                };

                Ok(BrainAction::ScheduleTask {
                    task,
                    target_node: raw.fields
                        .get("target_node")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    priority: raw.fields
                        .get("priority")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(5) as u8,
                })
            }
            "RebalanceTask" => Ok(BrainAction::RebalanceTask {
                task_id: raw.fields
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                to_node: raw.fields
                    .get("to_node")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            }),
            "CancelTask" => Ok(BrainAction::CancelTask {
                task_id: raw.fields
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            }),
            "UpdateGoalProgress" => Ok(BrainAction::UpdateGoalProgress {
                goal_id: raw.fields
                    .get("goal_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                progress_percent: raw.fields
                    .get("progress_percent")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u8,
                notes: raw.fields.get("notes").and_then(|v| v.as_str()).map(String::from),
            }),
            "MarkNodeDegraded" => Ok(BrainAction::MarkNodeDegraded {
                node_id: raw.fields
                    .get("node_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                reason: raw.fields
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            }),
            "RequestHumanApproval" => Ok(BrainAction::RequestHumanApproval {
                action_description: raw.fields
                    .get("action_description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                severity: raw.fields
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("medium")
                    .to_string(),
            }),
            "NoOp" => Ok(BrainAction::NoOp {
                reason: raw.fields
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            }),
            _ => Err(anyhow!("Unknown action type: {}", raw.action_type)),
        }
    }
}

#[async_trait]
impl Brain for LlmPlanner {
    async fn plan(
        &self,
        goals: &[Goal],
        cluster: &ClusterView,
        attachments: &[Attachment],
    ) -> Result<Vec<BrainAction>> {
        if goals.iter().filter(|g| g.active).count() == 0 {
            debug!("No active goals, skipping planning");
            return Ok(vec![BrainAction::NoOp {
                reason: "No active goals".to_string(),
            }]);
        }

        let input = Self::build_input(goals, cluster, attachments);
        let input_json = serde_json::to_string_pretty(&input)?;

        let user_msg = format!(
            "Current state:\n```json\n{}\n```\n\nAnalyze and propose actions.",
            input_json
        );

        let response = self.client.chat(SYSTEM_PROMPT, &user_msg).await?;
        debug!("LLM response: {}", response);

        let output: PlannerOutput = serde_json::from_str(&response)
            .map_err(|e| anyhow!("Failed to parse LLM output: {} - raw: {}", e, response))?;

        debug!("Planning reasoning: {}", output.reasoning);

        let mut actions = Vec::new();
        for raw_action in &output.actions {
            match Self::parse_action(raw_action) {
                Ok(action) => actions.push(action),
                Err(e) => {
                    warn!("Failed to parse action {:?}: {}", raw_action, e);
                }
            }
        }

        Ok(actions)
    }
}

pub struct NoOpBrain;

#[async_trait]
impl Brain for NoOpBrain {
    async fn plan(
        &self,
        _goals: &[Goal],
        _cluster: &ClusterView,
        _attachments: &[Attachment],
    ) -> Result<Vec<BrainAction>> {
        Ok(vec![BrainAction::NoOp {
            reason: "NoOp brain - planning disabled".to_string(),
        }])
    }
}
