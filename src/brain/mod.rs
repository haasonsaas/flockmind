mod llm_client;
mod planner;
pub mod tracker;

pub use llm_client::{LlmClient, LlmConfig};
pub use planner::*;
pub use tracker::*;

use crate::types::*;
use async_trait::async_trait;

#[async_trait]
pub trait Brain: Send + Sync {
    async fn plan(
        &self,
        goals: &[Goal],
        cluster: &ClusterView,
        attachments: &[Attachment],
    ) -> anyhow::Result<Vec<BrainAction>>;
}
