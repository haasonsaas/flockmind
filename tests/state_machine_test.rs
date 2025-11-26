use chrono::Utc;
use flockmind::replicator::state_machine::*;
use flockmind::*;

#[test]
fn test_hive_state_new() {
    let state = HiveState::new();
    assert!(state.nodes.is_empty());
    assert!(state.tasks.is_empty());
    assert!(state.attachments.is_empty());
    assert!(state.goals.is_empty());
    assert_eq!(state.last_applied_index, 0);
}

#[test]
fn test_apply_register_node() {
    let mut state = HiveState::new();
    let cmd = ClusterCommand::RegisterNode(NodeStatus {
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        tags: vec!["gpu".to_string()],
        health: NodeHealth::Healthy,
        last_heartbeat: Utc::now(),
        cpu_usage: 0.5,
        memory_usage: 0.3,
        disk_usage: 0.2,
    });

    state.apply(&cmd);

    assert_eq!(state.nodes.len(), 1);
    assert!(state.nodes.contains_key("node-1"));
    assert_eq!(state.nodes.get("node-1").unwrap().hostname, "host1");
}

#[test]
fn test_apply_update_node_health() {
    let mut state = HiveState::new();

    state.apply(&ClusterCommand::RegisterNode(NodeStatus {
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        tags: vec![],
        health: NodeHealth::Healthy,
        last_heartbeat: Utc::now(),
        cpu_usage: 0.0,
        memory_usage: 0.0,
        disk_usage: 0.0,
    }));

    state.apply(&ClusterCommand::UpdateNodeHealth {
        node_id: "node-1".to_string(),
        health: NodeHealth::Degraded {
            reason: "high cpu".to_string(),
        },
        metrics: NodeMetrics {
            cpu_usage: 0.95,
            memory_usage: 0.5,
            disk_usage: 0.3,
        },
    });

    let node = state.nodes.get("node-1").unwrap();
    assert!(matches!(node.health, NodeHealth::Degraded { .. }));
    assert_eq!(node.cpu_usage, 0.95);
}

#[test]
fn test_apply_remove_node() {
    let mut state = HiveState::new();

    state.apply(&ClusterCommand::RegisterNode(NodeStatus {
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        tags: vec![],
        health: NodeHealth::Healthy,
        last_heartbeat: Utc::now(),
        cpu_usage: 0.0,
        memory_usage: 0.0,
        disk_usage: 0.0,
    }));

    assert_eq!(state.nodes.len(), 1);

    state.apply(&ClusterCommand::RemoveNode {
        node_id: "node-1".to_string(),
    });

    assert!(state.nodes.is_empty());
}

#[test]
fn test_apply_put_task() {
    let mut state = HiveState::new();
    let task = Task {
        id: "task-1".to_string(),
        target_node: "node-1".to_string(),
        payload: TaskPayload::Echo {
            message: "hello".to_string(),
        },
        status: TaskStatus::Pending,
        priority: 5,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        result: None,
    };

    state.apply(&ClusterCommand::PutTask(task));

    assert_eq!(state.tasks.len(), 1);
    assert!(state.tasks.contains_key("task-1"));
}

#[test]
fn test_apply_update_task_status() {
    let mut state = HiveState::new();

    state.apply(&ClusterCommand::PutTask(Task {
        id: "task-1".to_string(),
        target_node: "node-1".to_string(),
        payload: TaskPayload::Echo {
            message: "hello".to_string(),
        },
        status: TaskStatus::Pending,
        priority: 5,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        result: None,
    }));

    state.apply(&ClusterCommand::UpdateTaskStatus {
        task_id: "task-1".to_string(),
        status: TaskStatus::Completed,
        result: Some(serde_json::json!({"output": "done"})),
    });

    let task = state.tasks.get("task-1").unwrap();
    assert_eq!(task.status, TaskStatus::Completed);
    assert!(task.result.is_some());
}

#[test]
fn test_apply_put_attachment() {
    let mut state = HiveState::new();
    let attachment = Attachment {
        id: "attach-1".to_string(),
        node_id: "node-1".to_string(),
        kind: AttachmentKind::Directory {
            path: "/data".to_string(),
        },
        capabilities: vec!["read".to_string(), "write".to_string()],
        metadata: std::collections::HashMap::new(),
        created_at: Utc::now(),
    };

    state.apply(&ClusterCommand::PutAttachment(attachment));

    assert_eq!(state.attachments.len(), 1);
    assert!(state.attachments.contains_key("attach-1"));
}

#[test]
fn test_apply_remove_attachment() {
    let mut state = HiveState::new();

    state.apply(&ClusterCommand::PutAttachment(Attachment {
        id: "attach-1".to_string(),
        node_id: "node-1".to_string(),
        kind: AttachmentKind::Directory {
            path: "/data".to_string(),
        },
        capabilities: vec![],
        metadata: std::collections::HashMap::new(),
        created_at: Utc::now(),
    }));

    assert_eq!(state.attachments.len(), 1);

    state.apply(&ClusterCommand::RemoveAttachment {
        attachment_id: "attach-1".to_string(),
    });

    assert!(state.attachments.is_empty());
}

#[test]
fn test_apply_put_goal() {
    let mut state = HiveState::new();
    let goal = Goal {
        id: "goal-1".to_string(),
        description: "Keep nginx running".to_string(),
        constraints: vec!["at least 2 replicas".to_string()],
        priority: 5,
        active: true,
        created_at: Utc::now(),
    };

    state.apply(&ClusterCommand::PutGoal(goal));

    assert_eq!(state.goals.len(), 1);
    assert!(state.goals.contains_key("goal-1"));
}

#[test]
fn test_apply_remove_goal() {
    let mut state = HiveState::new();

    state.apply(&ClusterCommand::PutGoal(Goal {
        id: "goal-1".to_string(),
        description: "Test".to_string(),
        constraints: vec![],
        priority: 5,
        active: true,
        created_at: Utc::now(),
    }));

    state.apply(&ClusterCommand::RemoveGoal {
        goal_id: "goal-1".to_string(),
    });

    assert!(state.goals.is_empty());
}

#[test]
fn test_shared_state_apply() {
    let shared = SharedState::new();

    shared.apply(&ClusterCommand::RegisterNode(NodeStatus {
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        tags: vec![],
        health: NodeHealth::Healthy,
        last_heartbeat: Utc::now(),
        cpu_usage: 0.0,
        memory_usage: 0.0,
        disk_usage: 0.0,
    }));

    let snapshot = shared.snapshot();
    assert_eq!(snapshot.nodes.len(), 1);
}

#[test]
fn test_shared_state_to_cluster_view() {
    let shared = SharedState::new();

    shared.apply(&ClusterCommand::RegisterNode(NodeStatus {
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        tags: vec![],
        health: NodeHealth::Healthy,
        last_heartbeat: Utc::now(),
        cpu_usage: 0.0,
        memory_usage: 0.0,
        disk_usage: 0.0,
    }));

    let view = shared.to_cluster_view(Some("node-1".to_string()), 5);
    assert_eq!(view.nodes.len(), 1);
    assert_eq!(view.leader_id, Some("node-1".to_string()));
    assert_eq!(view.term, 5);
}

#[test]
fn test_shared_state_restore() {
    let shared = SharedState::new();

    let mut state = HiveState::new();
    state.nodes.insert(
        "node-1".to_string(),
        NodeStatus {
            node_id: "node-1".to_string(),
            hostname: "host1".to_string(),
            tags: vec![],
            health: NodeHealth::Healthy,
            last_heartbeat: Utc::now(),
            cpu_usage: 0.0,
            memory_usage: 0.0,
            disk_usage: 0.0,
        },
    );
    state.last_applied_index = 100;

    shared.restore(state);

    let snapshot = shared.snapshot();
    assert_eq!(snapshot.nodes.len(), 1);
    assert_eq!(snapshot.last_applied_index, 100);
}

#[test]
fn test_shared_state_last_applied() {
    let shared = SharedState::new();
    assert_eq!(shared.last_applied(), 0);

    shared.set_last_applied(42);
    assert_eq!(shared.last_applied(), 42);
}

#[test]
fn test_shared_state_clone() {
    let shared = SharedState::new();
    shared.apply(&ClusterCommand::RegisterNode(NodeStatus {
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        tags: vec![],
        health: NodeHealth::Healthy,
        last_heartbeat: Utc::now(),
        cpu_usage: 0.0,
        memory_usage: 0.0,
        disk_usage: 0.0,
    }));

    let cloned = shared.clone();

    assert_eq!(shared.snapshot().nodes.len(), 1);
    assert_eq!(cloned.snapshot().nodes.len(), 1);

    cloned.apply(&ClusterCommand::RegisterNode(NodeStatus {
        node_id: "node-2".to_string(),
        hostname: "host2".to_string(),
        tags: vec![],
        health: NodeHealth::Healthy,
        last_heartbeat: Utc::now(),
        cpu_usage: 0.0,
        memory_usage: 0.0,
        disk_usage: 0.0,
    }));

    assert_eq!(shared.snapshot().nodes.len(), 2);
}
