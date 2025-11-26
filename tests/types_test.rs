use chrono::Utc;
use flockmind::*;

#[test]
fn test_cluster_view_new() {
    let view = ClusterView::new();
    assert!(view.nodes.is_empty());
    assert!(view.tasks.is_empty());
    assert!(view.attachments.is_empty());
    assert!(view.goals.is_empty());
    assert!(view.leader_id.is_none());
    assert_eq!(view.term, 0);
}

#[test]
fn test_cluster_view_node_by_id() {
    let mut view = ClusterView::new();
    view.nodes.push(NodeStatus {
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        tags: vec!["gpu".to_string()],
        health: NodeHealth::Healthy,
        last_heartbeat: Utc::now(),
        cpu_usage: 0.5,
        memory_usage: 0.3,
        disk_usage: 0.2,
    });

    assert!(view.node_by_id("node-1").is_some());
    assert!(view.node_by_id("node-2").is_none());
}

#[test]
fn test_cluster_view_healthy_nodes() {
    let mut view = ClusterView::new();
    view.nodes.push(NodeStatus {
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        tags: vec![],
        health: NodeHealth::Healthy,
        last_heartbeat: Utc::now(),
        cpu_usage: 0.0,
        memory_usage: 0.0,
        disk_usage: 0.0,
    });
    view.nodes.push(NodeStatus {
        node_id: "node-2".to_string(),
        hostname: "host2".to_string(),
        tags: vec![],
        health: NodeHealth::Degraded {
            reason: "high load".to_string(),
        },
        last_heartbeat: Utc::now(),
        cpu_usage: 0.9,
        memory_usage: 0.0,
        disk_usage: 0.0,
    });

    let healthy = view.healthy_nodes();
    assert_eq!(healthy.len(), 1);
    assert_eq!(healthy[0].node_id, "node-1");
}

#[test]
fn test_cluster_view_nodes_with_tag() {
    let mut view = ClusterView::new();
    view.nodes.push(NodeStatus {
        node_id: "node-1".to_string(),
        hostname: "host1".to_string(),
        tags: vec!["gpu".to_string(), "dev".to_string()],
        health: NodeHealth::Healthy,
        last_heartbeat: Utc::now(),
        cpu_usage: 0.0,
        memory_usage: 0.0,
        disk_usage: 0.0,
    });
    view.nodes.push(NodeStatus {
        node_id: "node-2".to_string(),
        hostname: "host2".to_string(),
        tags: vec!["cpu".to_string()],
        health: NodeHealth::Healthy,
        last_heartbeat: Utc::now(),
        cpu_usage: 0.0,
        memory_usage: 0.0,
        disk_usage: 0.0,
    });

    let gpu_nodes = view.nodes_with_tag("gpu");
    assert_eq!(gpu_nodes.len(), 1);
    assert_eq!(gpu_nodes[0].node_id, "node-1");
}

#[test]
fn test_cluster_view_pending_tasks() {
    let mut view = ClusterView::new();
    view.tasks.push(Task {
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
    });
    view.tasks.push(Task {
        id: "task-2".to_string(),
        target_node: "node-1".to_string(),
        payload: TaskPayload::Echo {
            message: "world".to_string(),
        },
        status: TaskStatus::Running,
        priority: 5,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        result: None,
    });

    let pending = view.pending_tasks();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "task-1");
}

#[test]
fn test_brain_action_serialization() {
    let action = BrainAction::ScheduleTask {
        task: TaskPayload::Echo {
            message: "test".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    let json = serde_json::to_string(&action).unwrap();
    let deserialized: BrainAction = serde_json::from_str(&json).unwrap();

    match deserialized {
        BrainAction::ScheduleTask {
            target_node,
            priority,
            ..
        } => {
            assert_eq!(target_node, "node-1");
            assert_eq!(priority, 5);
        }
        _ => panic!("Wrong variant"),
    }
}

#[test]
fn test_cluster_command_serialization() {
    let cmd = ClusterCommand::UpdateNodeHealth {
        node_id: "node-1".to_string(),
        health: NodeHealth::Healthy,
        metrics: NodeMetrics {
            cpu_usage: 0.5,
            memory_usage: 0.3,
            disk_usage: 0.2,
        },
    };

    let json = serde_json::to_string(&cmd).unwrap();
    let deserialized: ClusterCommand = serde_json::from_str(&json).unwrap();

    match deserialized {
        ClusterCommand::UpdateNodeHealth {
            node_id, metrics, ..
        } => {
            assert_eq!(node_id, "node-1");
            assert_eq!(metrics.cpu_usage, 0.5);
        }
        _ => panic!("Wrong variant"),
    }
}
