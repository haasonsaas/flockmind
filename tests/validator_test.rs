use chrono::Utc;
use flockmind::executor::ExecutionPolicy;
use flockmind::executor::validator::ActionValidator;
use flockmind::*;
use std::collections::HashMap;

fn create_test_policy() -> ExecutionPolicy {
    ExecutionPolicy {
        allow_restart_services: false,
        allow_docker: false,
        allowed_sync_paths: vec!["/home".to_string(), "/data".to_string()],
        blocked_sync_paths: vec!["/etc".to_string(), "/var".to_string()],
        require_approval_for_destructive: true,
        max_concurrent_tasks_per_node: 5,
    }
}

fn create_test_cluster_view() -> ClusterView {
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
    view.goals.push(Goal {
        id: "goal-1".to_string(),
        description: "Test goal".to_string(),
        constraints: vec![],
        priority: 5,
        active: true,
        created_at: Utc::now(),
    });
    view.attachments.push(Attachment {
        id: "attach-1".to_string(),
        node_id: "node-1".to_string(),
        kind: AttachmentKind::Directory {
            path: "/home/data".to_string(),
        },
        capabilities: vec![],
        metadata: HashMap::new(),
        created_at: Utc::now(),
    });
    view
}

#[test]
fn test_validate_echo_task() {
    let validator = ActionValidator::new(create_test_policy());
    let view = create_test_cluster_view();

    let action = BrainAction::ScheduleTask {
        task: TaskPayload::Echo {
            message: "hello".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    assert!(validator.validate(&action, &view).is_ok());
}

#[test]
fn test_validate_check_service_task() {
    let validator = ActionValidator::new(create_test_policy());
    let view = create_test_cluster_view();

    let action = BrainAction::ScheduleTask {
        task: TaskPayload::CheckService {
            service_name: "nginx".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    assert!(validator.validate(&action, &view).is_ok());
}

#[test]
fn test_validate_restart_service_blocked() {
    let validator = ActionValidator::new(create_test_policy());
    let view = create_test_cluster_view();

    let action = BrainAction::ScheduleTask {
        task: TaskPayload::RestartService {
            service_name: "nginx".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    let result = validator.validate(&action, &view);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not allowed"));
}

#[test]
fn test_validate_restart_service_allowed() {
    let mut policy = create_test_policy();
    policy.allow_restart_services = true;
    let validator = ActionValidator::new(policy);
    let view = create_test_cluster_view();

    let action = BrainAction::ScheduleTask {
        task: TaskPayload::RestartService {
            service_name: "nginx".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    assert!(validator.validate(&action, &view).is_ok());
}

#[test]
fn test_validate_docker_blocked() {
    let validator = ActionValidator::new(create_test_policy());
    let view = create_test_cluster_view();

    let action = BrainAction::ScheduleTask {
        task: TaskPayload::DockerRun {
            image: "nginx".to_string(),
            args: vec![],
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    let result = validator.validate(&action, &view);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Docker"));
}

#[test]
fn test_validate_sync_allowed_path() {
    let validator = ActionValidator::new(create_test_policy());
    let view = create_test_cluster_view();

    let action = BrainAction::ScheduleTask {
        task: TaskPayload::SyncDirectory {
            src: "/home/user/data".to_string(),
            dst: "/data/backup".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    assert!(validator.validate(&action, &view).is_ok());
}

#[test]
fn test_validate_sync_blocked_path() {
    let validator = ActionValidator::new(create_test_policy());
    let view = create_test_cluster_view();

    let action = BrainAction::ScheduleTask {
        task: TaskPayload::SyncDirectory {
            src: "/etc/nginx".to_string(),
            dst: "/home/backup".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    let result = validator.validate(&action, &view);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("blocked"));
}

#[test]
fn test_validate_run_command_blocked() {
    let validator = ActionValidator::new(create_test_policy());
    let view = create_test_cluster_view();

    let action = BrainAction::ScheduleTask {
        task: TaskPayload::RunCommand {
            command: "rm".to_string(),
            args: vec!["-rf".to_string(), "/".to_string()],
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    let result = validator.validate(&action, &view);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not allowed"));
}

#[test]
fn test_validate_unknown_node() {
    let validator = ActionValidator::new(create_test_policy());
    let view = create_test_cluster_view();

    let action = BrainAction::ScheduleTask {
        task: TaskPayload::Echo {
            message: "hello".to_string(),
        },
        target_node: "unknown-node".to_string(),
        priority: 5,
    };

    let result = validator.validate(&action, &view);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn test_validate_task_limit() {
    let mut policy = create_test_policy();
    policy.max_concurrent_tasks_per_node = 2;
    let validator = ActionValidator::new(policy);

    let mut view = create_test_cluster_view();
    for i in 0..2 {
        view.tasks.push(Task {
            id: format!("task-{}", i),
            target_node: "node-1".to_string(),
            payload: TaskPayload::Echo {
                message: "test".to_string(),
            },
            status: TaskStatus::Pending,
            priority: 5,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            result: None,
        });
    }

    let action = BrainAction::ScheduleTask {
        task: TaskPayload::Echo {
            message: "hello".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    let result = validator.validate(&action, &view);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("active tasks"));
}

#[test]
fn test_validate_cancel_task() {
    let validator = ActionValidator::new(create_test_policy());
    let mut view = create_test_cluster_view();
    view.tasks.push(Task {
        id: "task-1".to_string(),
        target_node: "node-1".to_string(),
        payload: TaskPayload::Echo {
            message: "test".to_string(),
        },
        status: TaskStatus::Running,
        priority: 5,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        result: None,
    });

    let action = BrainAction::CancelTask {
        task_id: "task-1".to_string(),
    };

    assert!(validator.validate(&action, &view).is_ok());
}

#[test]
fn test_validate_noop() {
    let validator = ActionValidator::new(create_test_policy());
    let view = create_test_cluster_view();

    let action = BrainAction::NoOp {
        reason: "nothing to do".to_string(),
    };

    assert!(validator.validate(&action, &view).is_ok());
}

#[test]
fn test_validate_request_human_approval() {
    let validator = ActionValidator::new(create_test_policy());
    let view = create_test_cluster_view();

    let action = BrainAction::RequestHumanApproval {
        action_description: "dangerous operation".to_string(),
        severity: "high".to_string(),
    };

    assert!(validator.validate(&action, &view).is_ok());
}

#[test]
fn test_default_policy() {
    let policy = ExecutionPolicy::default();
    assert!(!policy.allow_restart_services);
    assert!(!policy.allow_docker);
    assert!(policy.blocked_sync_paths.contains(&"/etc".to_string()));
}
