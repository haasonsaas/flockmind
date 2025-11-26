use crate::types::*;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct AttachmentRegistry {
    inner: Arc<RwLock<HashMap<AttachmentId, Attachment>>>,
    node_id: NodeId,
}

impl AttachmentRegistry {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            node_id,
        }
    }

    pub fn register(&self, kind: AttachmentKind, capabilities: Vec<String>) -> Attachment {
        let attachment = Attachment {
            id: Uuid::new_v4().to_string(),
            node_id: self.node_id.clone(),
            kind,
            capabilities,
            metadata: HashMap::new(),
            created_at: Utc::now(),
        };

        self.inner
            .write()
            .unwrap()
            .insert(attachment.id.clone(), attachment.clone());

        attachment
    }

    pub fn register_directory(&self, path: String, capabilities: Vec<String>) -> Attachment {
        self.register(AttachmentKind::Directory { path }, capabilities)
    }

    pub fn register_file(&self, path: String, capabilities: Vec<String>) -> Attachment {
        self.register(AttachmentKind::File { path }, capabilities)
    }

    pub fn register_service(&self, name: String, unit: Option<String>) -> Attachment {
        self.register(
            AttachmentKind::Service { name, unit },
            vec!["check_status".to_string()],
        )
    }

    pub fn register_docker(&self, container_id: String) -> Attachment {
        self.register(
            AttachmentKind::DockerContainer { container_id },
            vec![
                "start".to_string(),
                "stop".to_string(),
                "restart".to_string(),
                "logs".to_string(),
            ],
        )
    }

    pub fn register_webhook(&self, url: String) -> Attachment {
        self.register(
            AttachmentKind::Webhook { url },
            vec!["trigger".to_string()],
        )
    }

    pub fn unregister(&self, id: &str) -> Option<Attachment> {
        self.inner.write().unwrap().remove(id)
    }

    pub fn get(&self, id: &str) -> Option<Attachment> {
        self.inner.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<Attachment> {
        self.inner.read().unwrap().values().cloned().collect()
    }

    pub fn list_by_kind(&self, kind_name: &str) -> Vec<Attachment> {
        self.inner
            .read()
            .unwrap()
            .values()
            .filter(|a| {
                let k = match &a.kind {
                    AttachmentKind::Directory { .. } => "directory",
                    AttachmentKind::File { .. } => "file",
                    AttachmentKind::DockerContainer { .. } => "docker",
                    AttachmentKind::Service { .. } => "service",
                    AttachmentKind::Webhook { .. } => "webhook",
                    AttachmentKind::Custom { type_name, .. } => type_name,
                };
                k == kind_name
            })
            .cloned()
            .collect()
    }

    pub fn with_capability(&self, capability: &str) -> Vec<Attachment> {
        self.inner
            .read()
            .unwrap()
            .values()
            .filter(|a| a.capabilities.iter().any(|c| c == capability))
            .cloned()
            .collect()
    }

    pub fn set_metadata(&self, id: &str, key: String, value: String) -> bool {
        if let Some(attachment) = self.inner.write().unwrap().get_mut(id) {
            attachment.metadata.insert(key, value);
            true
        } else {
            false
        }
    }

    pub fn sync_from_cluster(&self, attachments: &[Attachment]) {
        let mut inner = self.inner.write().unwrap();
        inner.clear();
        for attachment in attachments {
            if attachment.node_id == self.node_id {
                inner.insert(attachment.id.clone(), attachment.clone());
            }
        }
    }
}

impl Clone for AttachmentRegistry {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            node_id: self.node_id.clone(),
        }
    }
}
