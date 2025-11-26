use crate::types::*;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedAction {
    pub id: String,
    pub action: BrainAction,
    pub proposed_at: DateTime<Utc>,
    pub status: ActionStatus,
    pub result: Option<ActionResult>,
    pub retry_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActionStatus {
    Proposed,
    Executing,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    pub message: Option<String>,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalProgress {
    pub goal_id: String,
    pub last_planned: DateTime<Utc>,
    pub actions_proposed: u32,
    pub actions_completed: u32,
    pub actions_failed: u32,
    pub estimated_progress: u8,
    pub notes: Vec<String>,
}

pub struct ActionTracker {
    actions: Arc<RwLock<HashMap<String, TrackedAction>>>,
    goal_progress: Arc<RwLock<HashMap<String, GoalProgress>>>,
    action_history: Arc<RwLock<Vec<TrackedAction>>>,
    max_history: usize,
    max_retries: u32,
    action_timeout: Duration,
}

impl ActionTracker {
    pub fn new() -> Self {
        Self {
            actions: Arc::new(RwLock::new(HashMap::new())),
            goal_progress: Arc::new(RwLock::new(HashMap::new())),
            action_history: Arc::new(RwLock::new(Vec::new())),
            max_history: 1000,
            max_retries: 3,
            action_timeout: Duration::minutes(5),
        }
    }

    pub fn track_action(&self, action: BrainAction) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let tracked = TrackedAction {
            id: id.clone(),
            action,
            proposed_at: Utc::now(),
            status: ActionStatus::Proposed,
            result: None,
            retry_count: 0,
        };

        self.actions.write().unwrap().insert(id.clone(), tracked);
        id
    }

    pub fn mark_executing(&self, id: &str) {
        if let Some(action) = self.actions.write().unwrap().get_mut(id) {
            action.status = ActionStatus::Executing;
        }
    }

    pub fn mark_completed(&self, id: &str, message: Option<String>) {
        let mut actions = self.actions.write().unwrap();
        if let Some(action) = actions.remove(id) {
            let mut completed = action;
            completed.status = ActionStatus::Completed;
            completed.result = Some(ActionResult {
                success: true,
                message,
                completed_at: Utc::now(),
            });

            self.add_to_history(completed);
        }
    }

    pub fn mark_failed(&self, id: &str, message: Option<String>) -> bool {
        let mut actions = self.actions.write().unwrap();
        if let Some(action) = actions.get_mut(id) {
            action.retry_count += 1;

            if action.retry_count >= self.max_retries {
                let mut failed = actions.remove(id).unwrap();
                failed.status = ActionStatus::Failed;
                failed.result = Some(ActionResult {
                    success: false,
                    message,
                    completed_at: Utc::now(),
                });
                drop(actions);
                self.add_to_history(failed);
                return false;
            } else {
                action.status = ActionStatus::Proposed;
                return true;
            }
        }
        false
    }

    fn add_to_history(&self, action: TrackedAction) {
        let mut history = self.action_history.write().unwrap();
        history.push(action);

        while history.len() > self.max_history {
            history.remove(0);
        }
    }

    pub fn get_pending_actions(&self) -> Vec<TrackedAction> {
        self.actions
            .read()
            .unwrap()
            .values()
            .filter(|a| a.status == ActionStatus::Proposed)
            .cloned()
            .collect()
    }

    pub fn cleanup_stale(&self) {
        let now = Utc::now();
        let mut actions = self.actions.write().unwrap();
        let stale_ids: Vec<_> = actions
            .iter()
            .filter(|(_, a)| {
                a.status == ActionStatus::Executing
                    && now - a.proposed_at > self.action_timeout
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in stale_ids {
            if let Some(mut action) = actions.remove(&id) {
                action.status = ActionStatus::Failed;
                action.result = Some(ActionResult {
                    success: false,
                    message: Some("Timeout".to_string()),
                    completed_at: now,
                });
                drop(actions);
                self.add_to_history(action);
                actions = self.actions.write().unwrap();
            }
        }
    }

    pub fn update_goal_progress(&self, goal_id: &str, completed: bool, notes: Option<String>) {
        let mut progress = self.goal_progress.write().unwrap();
        let entry = progress.entry(goal_id.to_string()).or_insert(GoalProgress {
            goal_id: goal_id.to_string(),
            last_planned: Utc::now(),
            actions_proposed: 0,
            actions_completed: 0,
            actions_failed: 0,
            estimated_progress: 0,
            notes: Vec::new(),
        });

        entry.last_planned = Utc::now();
        entry.actions_proposed += 1;

        if completed {
            entry.actions_completed += 1;
        } else {
            entry.actions_failed += 1;
        }

        if let Some(note) = notes {
            entry.notes.push(format!("[{}] {}", Utc::now().format("%H:%M:%S"), note));
            if entry.notes.len() > 50 {
                entry.notes.remove(0);
            }
        }

        let total = entry.actions_proposed.max(1);
        entry.estimated_progress =
            ((entry.actions_completed as f32 / total as f32) * 100.0) as u8;
    }

    pub fn get_goal_progress(&self, goal_id: &str) -> Option<GoalProgress> {
        self.goal_progress.read().unwrap().get(goal_id).cloned()
    }

    pub fn get_recent_failures(&self, limit: usize) -> Vec<TrackedAction> {
        self.action_history
            .read()
            .unwrap()
            .iter()
            .rev()
            .filter(|a| a.status == ActionStatus::Failed)
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn get_recent_actions(&self, limit: usize) -> Vec<TrackedAction> {
        self.action_history
            .read()
            .unwrap()
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn has_similar_pending(&self, action: &BrainAction) -> bool {
        self.actions
            .read()
            .unwrap()
            .values()
            .any(|a| is_similar_action(&a.action, action))
    }

    pub fn get_stats(&self) -> TrackerStats {
        let actions = self.actions.read().unwrap();
        let history = self.action_history.read().unwrap();

        let pending = actions.values().filter(|a| a.status == ActionStatus::Proposed).count();
        let executing = actions.values().filter(|a| a.status == ActionStatus::Executing).count();
        let completed = history.iter().filter(|a| a.status == ActionStatus::Completed).count();
        let failed = history.iter().filter(|a| a.status == ActionStatus::Failed).count();

        TrackerStats {
            pending,
            executing,
            completed,
            failed,
            history_size: history.len(),
        }
    }
}

impl Default for ActionTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackerStats {
    pub pending: usize,
    pub executing: usize,
    pub completed: usize,
    pub failed: usize,
    pub history_size: usize,
}

pub fn is_similar_action(a: &BrainAction, b: &BrainAction) -> bool {
    match (a, b) {
        (
            BrainAction::ScheduleTask { task: t1, target_node: n1, .. },
            BrainAction::ScheduleTask { task: t2, target_node: n2, .. },
        ) => n1 == n2 && std::mem::discriminant(t1) == std::mem::discriminant(t2),
        (
            BrainAction::RebalanceTask { task_id: t1, .. },
            BrainAction::RebalanceTask { task_id: t2, .. },
        ) => t1 == t2,
        (
            BrainAction::CancelTask { task_id: t1 },
            BrainAction::CancelTask { task_id: t2 },
        ) => t1 == t2,
        (
            BrainAction::MarkNodeDegraded { node_id: n1, .. },
            BrainAction::MarkNodeDegraded { node_id: n2, .. },
        ) => n1 == n2,
        _ => false,
    }
}
