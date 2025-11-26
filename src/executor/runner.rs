use crate::types::TaskPayload;
use anyhow::{anyhow, Result};
use serde_json::json;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, info, warn};

pub struct TaskRunner {
    timeout_secs: u64,
}

impl TaskRunner {
    pub fn new() -> Self {
        Self { timeout_secs: 300 }
    }

    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }

    pub async fn run(&self, task: &TaskPayload) -> Result<serde_json::Value> {
        match task {
            TaskPayload::Echo { message } => {
                info!("Echo: {}", message);
                Ok(json!({ "echoed": message }))
            }

            TaskPayload::CheckService { service_name } => {
                self.check_service(service_name).await
            }

            TaskPayload::RestartService { service_name } => {
                self.restart_service(service_name).await
            }

            TaskPayload::SyncDirectory { src, dst } => {
                self.sync_directory(src, dst).await
            }

            TaskPayload::DockerRun { image, args } => {
                self.docker_run(image, args).await
            }

            TaskPayload::RunCommand { .. } => {
                Err(anyhow!("Arbitrary command execution is disabled"))
            }

            TaskPayload::Custom { tool_id, .. } => {
                Err(anyhow!("Custom tool '{}' not implemented", tool_id))
            }
        }
    }

    async fn check_service(&self, service_name: &str) -> Result<serde_json::Value> {
        debug!("Checking service: {}", service_name);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("systemctl")
                .args(["is-active", service_name])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await??;

        let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let is_active = output.status.success() && status == "active";

        Ok(json!({
            "service": service_name,
            "status": status,
            "is_active": is_active
        }))
    }

    async fn restart_service(&self, service_name: &str) -> Result<serde_json::Value> {
        warn!("Restarting service: {}", service_name);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            Command::new("systemctl")
                .args(["restart", service_name])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to restart {}: {}", service_name, stderr));
        }

        Ok(json!({
            "service": service_name,
            "action": "restarted",
            "success": true
        }))
    }

    async fn sync_directory(&self, src: &str, dst: &str) -> Result<serde_json::Value> {
        info!("Syncing {} -> {}", src, dst);

        if !std::path::Path::new(src).exists() {
            return Err(anyhow!("Source path does not exist: {}", src));
        }

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            Command::new("rsync")
                .args(["-av", "--delete", src, dst])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("rsync failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(json!({
            "src": src,
            "dst": dst,
            "success": true,
            "output": stdout.lines().take(20).collect::<Vec<_>>().join("\n")
        }))
    }

    async fn docker_run(&self, image: &str, args: &[String]) -> Result<serde_json::Value> {
        info!("Docker run: {} {:?}", image, args);

        let mut cmd_args = vec!["run", "--rm"];
        for arg in args {
            cmd_args.push(arg);
        }
        cmd_args.push(image);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            Command::new("docker")
                .args(&cmd_args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await??;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Err(anyhow!("Docker run failed: {}", stderr));
        }

        Ok(json!({
            "image": image,
            "exit_code": output.status.code(),
            "stdout": stdout.lines().take(50).collect::<Vec<_>>().join("\n"),
            "stderr": stderr.lines().take(10).collect::<Vec<_>>().join("\n")
        }))
    }
}

impl Default for TaskRunner {
    fn default() -> Self {
        Self::new()
    }
}
