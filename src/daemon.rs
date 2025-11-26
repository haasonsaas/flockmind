use crate::attachments::AttachmentRegistry;
use crate::brain::{ActionTracker, Brain, LlmPlanner, NoOpBrain};
use crate::config::NodeConfig;
use crate::executor::{Executor, HiveExecutor};
use crate::replicator::{RaftReplicator, Replicator};
use crate::types::*;
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

pub struct HiveDaemon {
    node_id: String,
    hostname: String,
    tags: Vec<String>,
    replicator: Arc<RaftReplicator>,
    brain: Arc<dyn Brain>,
    executor: Arc<HiveExecutor<RaftReplicator>>,
    attachments: AttachmentRegistry,
    tracker: Arc<ActionTracker>,
    config: NodeConfig,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

impl HiveDaemon {
    pub async fn new(config: NodeConfig) -> Result<Self> {
        let node_id = config.effective_node_id();
        let hostname = config.effective_hostname();

        info!("Initializing HiveDaemon node_id={} hostname={}", node_id, hostname);

        let raft_node_id: u64 = {
            let bytes = node_id.as_bytes();
            let mut arr = [0u8; 8];
            for (i, b) in bytes.iter().take(8).enumerate() {
                arr[i] = *b;
            }
            u64::from_le_bytes(arr)
        };

        std::fs::create_dir_all(&config.data_dir)?;
        
        let replicator = Arc::new(
            RaftReplicator::new(
                raft_node_id,
                config.listen_addr(),
                hostname.clone(),
                &config.data_dir,
            )
            .await?,
        );

        let brain: Arc<dyn Brain> = if config.llm.enabled {
            let llm_config = config.llm.to_llm_config();
            if llm_config.api_key.is_empty() {
                warn!("LLM enabled but API key is empty, using NoOpBrain");
                Arc::new(NoOpBrain)
            } else {
                Arc::new(LlmPlanner::new(llm_config)?)
            }
        } else {
            Arc::new(NoOpBrain)
        };

        let policy = config.policy.to_execution_policy();
        let executor = Arc::new(HiveExecutor::new(
            node_id.clone(),
            replicator.clone(),
            policy,
        ));

        let attachments = AttachmentRegistry::new(node_id.clone());
        let tracker = Arc::new(ActionTracker::new());

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Ok(Self {
            node_id,
            hostname,
            tags: config.tags.clone(),
            replicator,
            brain,
            executor,
            attachments,
            tracker,
            config,
            shutdown_tx,
            shutdown_rx,
        })
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting HiveDaemon...");

        if self.config.peers.is_empty() {
            info!("No peers configured, initializing as single-node cluster");
            self.replicator.initialize_single().await?;
        }

        self.register_self().await?;

        let heartbeat_handle = self.spawn_heartbeat_loop();
        let task_runner_handle = self.spawn_task_runner_loop();
        let planner_handle = self.spawn_planner_loop();

        info!("HiveDaemon running on {}", self.config.listen_addr());

        tokio::select! {
            _ = heartbeat_handle => {
                error!("Heartbeat loop exited unexpectedly");
            }
            _ = task_runner_handle => {
                error!("Task runner loop exited unexpectedly");
            }
            _ = planner_handle => {
                error!("Planner loop exited unexpectedly");
            }
            _ = self.wait_for_shutdown() => {
                info!("Shutdown signal received");
            }
        }

        Ok(())
    }

    async fn register_self(&self) -> Result<()> {
        let status = NodeStatus {
            node_id: self.node_id.clone(),
            hostname: self.hostname.clone(),
            tags: self.tags.clone(),
            health: NodeHealth::Healthy,
            last_heartbeat: Utc::now(),
            cpu_usage: 0.0,
            memory_usage: 0.0,
            disk_usage: 0.0,
        };

        self.replicator
            .apply(ClusterCommand::RegisterNode(status))
            .await?;

        info!("Registered node {} in cluster", self.node_id);
        Ok(())
    }

    fn spawn_heartbeat_loop(&self) -> tokio::task::JoinHandle<()> {
        let replicator = self.replicator.clone();
        let node_id = self.node_id.clone();
        let interval = self.config.heartbeat_interval_secs;
        let mut shutdown_rx = self.shutdown_rx.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let metrics = collect_node_metrics();
                        if let Err(e) = replicator
                            .apply(ClusterCommand::UpdateNodeHealth {
                                node_id: node_id.clone(),
                                health: NodeHealth::Healthy,
                                metrics,
                            })
                            .await
                        {
                            warn!("Failed to send heartbeat: {}", e);
                        } else {
                            debug!("Heartbeat sent");
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        break;
                    }
                }
            }
        })
    }

    fn spawn_task_runner_loop(&self) -> tokio::task::JoinHandle<()> {
        let replicator = self.replicator.clone();
        let executor = self.executor.clone();
        let node_id = self.node_id.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(2));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let view = replicator.snapshot();

                        let my_pending_tasks: Vec<_> = view
                            .tasks
                            .iter()
                            .filter(|t| {
                                t.target_node == node_id && t.status == TaskStatus::Pending
                            })
                            .cloned()
                            .collect();

                        for task in my_pending_tasks {
                            info!("Executing task {}: {:?}", task.id, task.payload);
                            match executor.run_task(&task).await {
                                Ok(result) => {
                                    info!("Task {} completed: {:?}", task.id, result);
                                }
                                Err(e) => {
                                    error!("Task {} failed: {}", task.id, e);
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        break;
                    }
                }
            }
        })
    }

    fn spawn_planner_loop(&self) -> tokio::task::JoinHandle<()> {
        let replicator = self.replicator.clone();
        let brain = self.brain.clone();
        let executor = self.executor.clone();
        let attachments = self.attachments.clone();
        let tracker = self.tracker.clone();
        let interval = self.config.planning_interval_secs;
        let mut shutdown_rx = self.shutdown_rx.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        tracker.cleanup_stale();

                        if !replicator.is_leader() {
                            debug!("Not leader, skipping planning");
                            continue;
                        }

                        let view = replicator.snapshot();
                        let attachment_list = attachments.list();

                        if view.goals.is_empty() {
                            debug!("No goals defined, skipping planning");
                            continue;
                        }

                        let recent_failures = tracker.get_recent_failures(10);
                        let stats = tracker.get_stats();
                        debug!(
                            "Tracker stats: pending={}, executing={}, completed={}, failed={}",
                            stats.pending, stats.executing, stats.completed, stats.failed
                        );

                        match brain.plan(&view.goals, &view, &attachment_list).await {
                            Ok(actions) => {
                                for action in actions {
                                    if tracker.has_similar_pending(&action) {
                                        debug!("Skipping duplicate action: {:?}", action);
                                        continue;
                                    }

                                    if is_recently_failed(&action, &recent_failures) {
                                        debug!("Skipping recently failed action: {:?}", action);
                                        continue;
                                    }

                                    let action_id = tracker.track_action(action.clone());
                                    tracker.mark_executing(&action_id);

                                    debug!("Executing brain action {}: {:?}", action_id, action);
                                    
                                    let goal_id = extract_goal_id(&action);
                                    
                                    match executor.execute(action).await {
                                        Ok(()) => {
                                            tracker.mark_completed(&action_id, None);
                                            if let Some(gid) = goal_id {
                                                tracker.update_goal_progress(&gid, true, None);
                                            }
                                        }
                                        Err(e) => {
                                            let msg = e.to_string();
                                            warn!("Failed to execute action {}: {}", action_id, msg);
                                            let should_retry = tracker.mark_failed(&action_id, Some(msg.clone()));
                                            if let Some(gid) = goal_id {
                                                tracker.update_goal_progress(&gid, false, Some(msg));
                                            }
                                            if !should_retry {
                                                warn!("Action {} exceeded max retries", action_id);
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Planning failed: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        break;
                    }
                }
            }
        })
    }

    pub fn tracker(&self) -> &Arc<ActionTracker> {
        &self.tracker
    }

    async fn wait_for_shutdown(&self) {
        let mut rx = self.shutdown_rx.clone();
        while !*rx.borrow() {
            let _ = rx.changed().await;
        }
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn replicator(&self) -> &Arc<RaftReplicator> {
        &self.replicator
    }

    pub fn attachments(&self) -> &AttachmentRegistry {
        &self.attachments
    }
}

fn collect_node_metrics() -> NodeMetrics {
    NodeMetrics {
        cpu_usage: 0.0,
        memory_usage: 0.0,
        disk_usage: 0.0,
    }
}

fn extract_goal_id(action: &BrainAction) -> Option<String> {
    match action {
        BrainAction::UpdateGoalProgress { goal_id, .. } => Some(goal_id.clone()),
        _ => None,
    }
}

fn is_recently_failed(action: &BrainAction, recent_failures: &[crate::brain::TrackedAction]) -> bool {
    use crate::brain::tracker::is_similar_action;
    recent_failures.iter().any(|f| is_similar_action(&f.action, action))
}
