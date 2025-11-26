mod network;
mod raft_node;
mod state_machine;
mod storage;

pub use network::*;
pub use raft_node::*;
pub use state_machine::*;
pub use storage::*;

use crate::types::*;
use async_trait::async_trait;

#[async_trait]
pub trait Replicator: Send + Sync {
    async fn apply(&self, command: ClusterCommand) -> anyhow::Result<()>;
    fn snapshot(&self) -> ClusterView;
    fn is_leader(&self) -> bool;
    fn leader_id(&self) -> Option<NodeId>;
    async fn add_peer(&self, peer: PeerInfo) -> anyhow::Result<()>;
}
