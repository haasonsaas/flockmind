use crate::auth::certs::{CaCertificate, NodeCertificate};
use crate::types::EnrollmentToken;
use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentRequest {
    pub token: String,
    pub node_id: String,
    pub hostname: String,
    pub hostnames: Vec<String>,
    pub ips: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentResponse {
    pub node_id: String,
    pub cluster_id: String,
    pub node_cert_pem: String,
    pub node_key_pem: String,
    pub ca_cert_pem: String,
    pub peers: Vec<PeerEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerEndpoint {
    pub node_id: String,
    pub addr: String,
}

pub struct EnrollmentManager {
    cluster_id: String,
    ca: CaCertificate,
    tokens: Arc<RwLock<HashMap<String, EnrollmentToken>>>,
    enrolled_nodes: Arc<RwLock<HashMap<String, EnrolledNode>>>,
}

#[derive(Debug, Clone)]
pub struct EnrolledNode {
    pub node_id: String,
    pub hostname: String,
    pub addr: String,
    pub tags: Vec<String>,
    pub enrolled_at: chrono::DateTime<Utc>,
}

impl EnrollmentManager {
    pub fn new(cluster_id: String, ca: CaCertificate) -> Self {
        Self {
            cluster_id,
            ca,
            tokens: Arc::new(RwLock::new(HashMap::new())),
            enrolled_nodes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn load_or_create<P: AsRef<Path>>(data_dir: P, cluster_id: &str) -> Result<Self> {
        let ca_cert_path = data_dir.as_ref().join("ca.crt");
        let ca_key_path = data_dir.as_ref().join("ca.key");

        let ca = if ca_cert_path.exists() && ca_key_path.exists() {
            tracing::info!("Loading existing CA certificate");
            CaCertificate::load(&ca_cert_path, &ca_key_path)?
        } else {
            tracing::info!("Generating new CA certificate for cluster {}", cluster_id);
            let ca = CaCertificate::generate(cluster_id)?;
            ca.save(&ca_cert_path, &ca_key_path)?;
            ca
        };

        Ok(Self::new(cluster_id.to_string(), ca))
    }

    pub fn generate_token(&self, valid_hours: i64, allowed_tags: Vec<String>) -> EnrollmentToken {
        let token: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();

        let enrollment_token = EnrollmentToken {
            token: token.clone(),
            cluster_id: self.cluster_id.clone(),
            expires_at: Utc::now() + Duration::hours(valid_hours),
            allowed_tags,
        };

        self.tokens
            .write()
            .unwrap()
            .insert(token, enrollment_token.clone());

        enrollment_token
    }

    pub fn enroll(&self, req: EnrollmentRequest) -> Result<EnrollmentResponse> {
        let token = self
            .tokens
            .read()
            .unwrap()
            .get(&req.token)
            .cloned()
            .ok_or_else(|| anyhow!("Invalid enrollment token"))?;

        if Utc::now() > token.expires_at {
            self.tokens.write().unwrap().remove(&req.token);
            return Err(anyhow!("Enrollment token has expired"));
        }

        if !token.allowed_tags.is_empty() {
            let has_allowed_tag = req.tags.iter().any(|t| token.allowed_tags.contains(t));
            if !has_allowed_tag {
                return Err(anyhow!(
                    "Node tags {:?} not in allowed tags {:?}",
                    req.tags,
                    token.allowed_tags
                ));
            }
        }

        let node_cert = self.ca.sign_node(&req.node_id, req.hostnames, req.ips)?;

        let peers: Vec<PeerEndpoint> = self
            .enrolled_nodes
            .read()
            .unwrap()
            .values()
            .map(|n| PeerEndpoint {
                node_id: n.node_id.clone(),
                addr: n.addr.clone(),
            })
            .collect();

        self.tokens.write().unwrap().remove(&req.token);

        Ok(EnrollmentResponse {
            node_id: req.node_id,
            cluster_id: self.cluster_id.clone(),
            node_cert_pem: node_cert.cert_pem,
            node_key_pem: node_cert.key_pem,
            ca_cert_pem: self.ca.cert_pem.clone(),
            peers,
        })
    }

    pub fn register_enrolled_node(&self, node_id: String, hostname: String, addr: String, tags: Vec<String>) {
        self.enrolled_nodes.write().unwrap().insert(
            node_id.clone(),
            EnrolledNode {
                node_id,
                hostname,
                addr,
                tags,
                enrolled_at: Utc::now(),
            },
        );
    }

    pub fn is_enrolled(&self, node_id: &str) -> bool {
        self.enrolled_nodes.read().unwrap().contains_key(node_id)
    }

    pub fn get_enrolled_nodes(&self) -> Vec<EnrolledNode> {
        self.enrolled_nodes.read().unwrap().values().cloned().collect()
    }

    pub fn ca_cert_pem(&self) -> &str {
        &self.ca.cert_pem
    }

    pub fn cluster_id(&self) -> &str {
        &self.cluster_id
    }

    pub fn sign_node_cert(
        &self,
        node_id: &str,
        hostnames: Vec<String>,
        ips: Vec<String>,
    ) -> Result<NodeCertificate> {
        self.ca.sign_node(node_id, hostnames, ips)
    }
}
