use chrono::Utc;
use flockmind::*;
use std::collections::HashMap;

#[test]
fn test_registry_new() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    assert!(registry.list().is_empty());
}

#[test]
fn test_register_directory() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    let attachment = registry.register_directory(
        "/data/files".to_string(),
        vec!["read".to_string(), "write".to_string()],
    );

    assert!(!attachment.id.is_empty());
    assert_eq!(attachment.node_id, "node-1");
    assert!(matches!(attachment.kind, AttachmentKind::Directory { .. }));
    assert_eq!(attachment.capabilities.len(), 2);
}

#[test]
fn test_register_file() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    let attachment =
        registry.register_file("/etc/config.toml".to_string(), vec!["read".to_string()]);

    assert!(matches!(attachment.kind, AttachmentKind::File { .. }));
}

#[test]
fn test_register_service() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    let attachment =
        registry.register_service("nginx".to_string(), Some("nginx.service".to_string()));

    assert!(matches!(attachment.kind, AttachmentKind::Service { .. }));
    assert!(attachment.capabilities.contains(&"check_status".to_string()));
}

#[test]
fn test_register_docker() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    let attachment = registry.register_docker("abc123".to_string());

    assert!(matches!(
        attachment.kind,
        AttachmentKind::DockerContainer { .. }
    ));
    assert!(attachment.capabilities.contains(&"start".to_string()));
    assert!(attachment.capabilities.contains(&"stop".to_string()));
}

#[test]
fn test_register_webhook() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    let attachment = registry.register_webhook("https://example.com/hook".to_string());

    assert!(matches!(attachment.kind, AttachmentKind::Webhook { .. }));
    assert!(attachment.capabilities.contains(&"trigger".to_string()));
}

#[test]
fn test_unregister() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    let attachment = registry.register_directory("/data".to_string(), vec![]);
    let id = attachment.id.clone();

    assert!(registry.get(&id).is_some());

    let removed = registry.unregister(&id);
    assert!(removed.is_some());
    assert!(registry.get(&id).is_none());
}

#[test]
fn test_get() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    let attachment = registry.register_directory("/data".to_string(), vec![]);

    let retrieved = registry.get(&attachment.id).unwrap();
    assert_eq!(retrieved.id, attachment.id);
}

#[test]
fn test_list() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    registry.register_directory("/data1".to_string(), vec![]);
    registry.register_directory("/data2".to_string(), vec![]);
    registry.register_file("/file".to_string(), vec![]);

    assert_eq!(registry.list().len(), 3);
}

#[test]
fn test_list_by_kind() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    registry.register_directory("/data1".to_string(), vec![]);
    registry.register_directory("/data2".to_string(), vec![]);
    registry.register_file("/file".to_string(), vec![]);
    registry.register_service("nginx".to_string(), None);

    let dirs = registry.list_by_kind("directory");
    assert_eq!(dirs.len(), 2);

    let files = registry.list_by_kind("file");
    assert_eq!(files.len(), 1);

    let services = registry.list_by_kind("service");
    assert_eq!(services.len(), 1);
}

#[test]
fn test_with_capability() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    registry.register_directory("/data1".to_string(), vec!["read".to_string()]);
    registry.register_directory(
        "/data2".to_string(),
        vec!["read".to_string(), "write".to_string()],
    );
    registry.register_file("/file".to_string(), vec!["execute".to_string()]);

    let readable = registry.with_capability("read");
    assert_eq!(readable.len(), 2);

    let writable = registry.with_capability("write");
    assert_eq!(writable.len(), 1);

    let executable = registry.with_capability("execute");
    assert_eq!(executable.len(), 1);
}

#[test]
fn test_set_metadata() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    let attachment = registry.register_directory("/data".to_string(), vec![]);

    let result = registry.set_metadata(&attachment.id, "key".to_string(), "value".to_string());
    assert!(result);

    let retrieved = registry.get(&attachment.id).unwrap();
    assert_eq!(retrieved.metadata.get("key"), Some(&"value".to_string()));
}

#[test]
fn test_set_metadata_nonexistent() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    let result = registry.set_metadata("nonexistent", "key".to_string(), "value".to_string());
    assert!(!result);
}

#[test]
fn test_sync_from_cluster() {
    let registry = AttachmentRegistry::new("node-1".to_string());

    registry.register_directory("/local".to_string(), vec![]);
    assert_eq!(registry.list().len(), 1);

    let cluster_attachments = vec![
        Attachment {
            id: "attach-1".to_string(),
            node_id: "node-1".to_string(),
            kind: AttachmentKind::Directory {
                path: "/data".to_string(),
            },
            capabilities: vec![],
            metadata: HashMap::new(),
            created_at: Utc::now(),
        },
        Attachment {
            id: "attach-2".to_string(),
            node_id: "node-2".to_string(),
            kind: AttachmentKind::Directory {
                path: "/other".to_string(),
            },
            capabilities: vec![],
            metadata: HashMap::new(),
            created_at: Utc::now(),
        },
    ];

    registry.sync_from_cluster(&cluster_attachments);

    let list = registry.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "attach-1");
}

#[test]
fn test_registry_clone() {
    let registry = AttachmentRegistry::new("node-1".to_string());
    registry.register_directory("/data".to_string(), vec![]);

    let cloned = registry.clone();

    assert_eq!(registry.list().len(), 1);
    assert_eq!(cloned.list().len(), 1);

    cloned.register_file("/file".to_string(), vec![]);
    assert_eq!(registry.list().len(), 2);
}
