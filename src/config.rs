use crate::brain::LlmConfig;
use crate::executor::ExecutionPolicy;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub node_id: Option<String>,
    pub hostname: Option<String>,
    pub tags: Vec<String>,

    pub bind_addr: String,
    pub bind_port: u16,

    pub data_dir: PathBuf,

    pub peers: Vec<PeerConfig>,

    pub llm: LlmSettings,

    pub policy: PolicySettings,

    pub heartbeat_interval_secs: u64,
    pub planning_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfig {
    pub node_id: String,
    pub addr: String,
    pub is_voter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSettings {
    pub enabled: bool,
    pub api_base: Option<String>,
    pub api_key_env: String,
    pub model: String,
    pub max_tokens: u16,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySettings {
    pub allow_restart_services: bool,
    pub allow_docker: bool,
    pub allowed_sync_paths: Vec<String>,
    pub blocked_sync_paths: Vec<String>,
    pub require_approval_for_destructive: bool,
    pub max_concurrent_tasks_per_node: usize,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            node_id: None,
            hostname: None,
            tags: Vec::new(),
            bind_addr: "0.0.0.0".to_string(),
            bind_port: 9000,
            data_dir: PathBuf::from("/var/lib/flockmind"),
            peers: Vec::new(),
            llm: LlmSettings::default(),
            policy: PolicySettings::default(),
            heartbeat_interval_secs: 10,
            planning_interval_secs: 30,
        }
    }
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            api_base: None,
            api_key_env: "OPENAI_API_KEY".to_string(),
            model: "gpt-4o-mini".to_string(),
            max_tokens: 2048,
            temperature: 0.1,
        }
    }
}

impl Default for PolicySettings {
    fn default() -> Self {
        Self {
            allow_restart_services: false,
            allow_docker: false,
            allowed_sync_paths: vec!["/home".to_string(), "/data".to_string()],
            blocked_sync_paths: vec![
                "/etc".to_string(),
                "/var".to_string(),
                "/usr".to_string(),
                "/bin".to_string(),
                "/sbin".to_string(),
                "/root".to_string(),
            ],
            require_approval_for_destructive: true,
            max_concurrent_tasks_per_node: 5,
        }
    }
}

impl LlmSettings {
    pub fn to_llm_config(&self) -> LlmConfig {
        let api_key = std::env::var(&self.api_key_env).unwrap_or_default();
        LlmConfig {
            api_key,
            api_base: self.api_base.clone(),
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            temperature: self.temperature,
        }
    }
}

impl PolicySettings {
    pub fn to_execution_policy(&self) -> ExecutionPolicy {
        ExecutionPolicy {
            allow_restart_services: self.allow_restart_services,
            allow_docker: self.allow_docker,
            allowed_sync_paths: self.allowed_sync_paths.clone(),
            blocked_sync_paths: self.blocked_sync_paths.clone(),
            require_approval_for_destructive: self.require_approval_for_destructive,
            max_concurrent_tasks_per_node: self.max_concurrent_tasks_per_node,
        }
    }
}

impl NodeConfig {
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, path: &PathBuf) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn effective_node_id(&self) -> String {
        self.node_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    }

    pub fn effective_hostname(&self) -> String {
        self.hostname.clone().unwrap_or_else(|| {
            hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "unknown".to_string())
        })
    }

    pub fn listen_addr(&self) -> String {
        format!("{}:{}", self.bind_addr, self.bind_port)
    }
}
