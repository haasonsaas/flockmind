use flockmind::brain::tracker::*;
use flockmind::*;

#[test]
fn test_action_tracker_new() {
    let tracker = ActionTracker::new();
    let stats = tracker.get_stats();
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.executing, 0);
    assert_eq!(stats.completed, 0);
    assert_eq!(stats.failed, 0);
}

#[test]
fn test_track_action() {
    let tracker = ActionTracker::new();
    let action = BrainAction::NoOp {
        reason: "test".to_string(),
    };

    let id = tracker.track_action(action);
    assert!(!id.is_empty());

    let stats = tracker.get_stats();
    assert_eq!(stats.pending, 1);
}

#[test]
fn test_mark_executing() {
    let tracker = ActionTracker::new();
    let action = BrainAction::NoOp {
        reason: "test".to_string(),
    };

    let id = tracker.track_action(action);
    tracker.mark_executing(&id);

    let stats = tracker.get_stats();
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.executing, 1);
}

#[test]
fn test_mark_completed() {
    let tracker = ActionTracker::new();
    let action = BrainAction::NoOp {
        reason: "test".to_string(),
    };

    let id = tracker.track_action(action);
    tracker.mark_executing(&id);
    tracker.mark_completed(&id, Some("done".to_string()));

    let stats = tracker.get_stats();
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.executing, 0);
    assert_eq!(stats.completed, 1);
}

#[test]
fn test_mark_failed_with_retry() {
    let tracker = ActionTracker::new();
    let action = BrainAction::NoOp {
        reason: "test".to_string(),
    };

    let id = tracker.track_action(action);
    tracker.mark_executing(&id);

    let should_retry = tracker.mark_failed(&id, Some("error".to_string()));
    assert!(should_retry);

    let stats = tracker.get_stats();
    assert_eq!(stats.pending, 1);
}

#[test]
fn test_mark_failed_max_retries() {
    let tracker = ActionTracker::new();
    let action = BrainAction::NoOp {
        reason: "test".to_string(),
    };

    let id = tracker.track_action(action);

    for i in 0..3 {
        tracker.mark_executing(&id);
        let should_retry = tracker.mark_failed(&id, Some(format!("error {}", i)));
        if i < 2 {
            assert!(should_retry);
        } else {
            assert!(!should_retry);
        }
    }

    let stats = tracker.get_stats();
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.failed, 1);
}

#[test]
fn test_has_similar_pending() {
    let tracker = ActionTracker::new();

    let action1 = BrainAction::ScheduleTask {
        task: TaskPayload::Echo {
            message: "hello".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    tracker.track_action(action1);

    let similar = BrainAction::ScheduleTask {
        task: TaskPayload::Echo {
            message: "world".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 3,
    };

    assert!(tracker.has_similar_pending(&similar));

    let different = BrainAction::ScheduleTask {
        task: TaskPayload::Echo {
            message: "hello".to_string(),
        },
        target_node: "node-2".to_string(),
        priority: 5,
    };

    assert!(!tracker.has_similar_pending(&different));
}

#[test]
fn test_goal_progress_tracking() {
    let tracker = ActionTracker::new();

    tracker.update_goal_progress("goal-1", true, Some("step 1 done".to_string()));
    tracker.update_goal_progress("goal-1", true, Some("step 2 done".to_string()));
    tracker.update_goal_progress("goal-1", false, Some("step 3 failed".to_string()));

    let progress = tracker.get_goal_progress("goal-1").unwrap();
    assert_eq!(progress.actions_proposed, 3);
    assert_eq!(progress.actions_completed, 2);
    assert_eq!(progress.actions_failed, 1);
    assert_eq!(progress.notes.len(), 3);
}

#[test]
fn test_is_similar_action_schedule_task() {
    let a = BrainAction::ScheduleTask {
        task: TaskPayload::Echo {
            message: "a".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    let b = BrainAction::ScheduleTask {
        task: TaskPayload::Echo {
            message: "b".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 3,
    };

    let c = BrainAction::ScheduleTask {
        task: TaskPayload::CheckService {
            service_name: "nginx".to_string(),
        },
        target_node: "node-1".to_string(),
        priority: 5,
    };

    assert!(is_similar_action(&a, &b));
    assert!(!is_similar_action(&a, &c));
}

#[test]
fn test_is_similar_action_cancel_task() {
    let a = BrainAction::CancelTask {
        task_id: "task-1".to_string(),
    };
    let b = BrainAction::CancelTask {
        task_id: "task-1".to_string(),
    };
    let c = BrainAction::CancelTask {
        task_id: "task-2".to_string(),
    };

    assert!(is_similar_action(&a, &b));
    assert!(!is_similar_action(&a, &c));
}

#[test]
fn test_tracker_stats_serialization() {
    let stats = TrackerStats {
        pending: 1,
        executing: 2,
        completed: 3,
        failed: 4,
        history_size: 100,
    };
    let json = serde_json::to_string(&stats).unwrap();
    assert!(json.contains("\"pending\":1"));
    assert!(json.contains("\"completed\":3"));
}
