use crate::replicator::network::HiveNetworkFactory;
use crate::replicator::state_machine::SharedState;
use crate::replicator::storage::{create_storage, HiveNode, NodeIdType, TypeConfig};
use crate::replicator::Replicator;
use crate::types::*;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use openraft::{ChangeMembers, Config, Raft};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

pub type HiveRaft = Raft<TypeConfig>;

pub struct RaftReplicator {
    node_id: NodeIdType,
    raft: HiveRaft,
    state: SharedState,
    network: HiveNetworkFactory,
}

impl RaftReplicator {
    pub async fn new<P: AsRef<Path>>(
        node_id: NodeIdType,
        addr: String,
        _hostname: String,
        data_dir: P,
    ) -> Result<Self> {
        let config = Config {
            heartbeat_interval: 500,
            election_timeout_min: 1500,
            election_timeout_max: 3000,
            ..Default::default()
        };
        let config = Arc::new(config.validate()?);

        let state = SharedState::new();
        let storage_path = data_dir.as_ref().join("raft");
        std::fs::create_dir_all(&storage_path)?;
        let (log_store, sm_store) = create_storage(&storage_path, state.clone())?;
        let network = HiveNetworkFactory::new();

        network.register_node(node_id, addr.clone());

        let raft = Raft::new(node_id, config, network.clone(), log_store, sm_store).await?;

        info!("Raft node {} initialized at {} with storage at {:?}", node_id, addr, storage_path);

        Ok(Self {
            node_id,
            raft,
            state,
            network,
        })
    }

    pub async fn initialize_single(&self) -> Result<()> {
        let mut members = BTreeMap::new();
        members.insert(
            self.node_id,
            HiveNode {
                addr: "127.0.0.1:9000".to_string(),
                hostname: "localhost".to_string(),
            },
        );
        self.raft.initialize(members).await?;
        Ok(())
    }

    pub fn raft(&self) -> &HiveRaft {
        &self.raft
    }

    pub fn node_id(&self) -> NodeIdType {
        self.node_id
    }

    pub fn shared_state(&self) -> &SharedState {
        &self.state
    }

    pub fn network(&self) -> &HiveNetworkFactory {
        &self.network
    }
}

#[async_trait]
impl Replicator for RaftReplicator {
    async fn apply(&self, command: ClusterCommand) -> Result<()> {
        let _resp = self
            .raft
            .client_write(command)
            .await
            .map_err(|e| anyhow!("Raft write failed: {}", e))?;
        Ok(())
    }

    fn snapshot(&self) -> ClusterView {
        let metrics = self.raft.metrics().borrow().clone();
        let leader_id = metrics.current_leader.map(|id| id.to_string());
        let term = metrics.current_term;
        self.state.to_cluster_view(leader_id, term)
    }

    fn is_leader(&self) -> bool {
        let metrics = self.raft.metrics().borrow().clone();
        metrics.current_leader == Some(self.node_id)
    }

    fn leader_id(&self) -> Option<NodeId> {
        let metrics = self.raft.metrics().borrow().clone();
        metrics.current_leader.map(|id| id.to_string())
    }

    async fn add_peer(&self, peer: PeerInfo) -> Result<()> {
        let node_id: NodeIdType = peer.node_id.parse()?;
        let node = HiveNode {
            addr: peer.addr.clone(),
            hostname: peer.node_id.clone(),
        };

        self.network.register_node(node_id, peer.addr.clone());

        if peer.is_voter {
            let mut members = BTreeMap::new();
            members.insert(node_id, node);
            self.raft
                .change_membership(ChangeMembers::AddNodes(members), false)
                .await?;
        } else {
            self.raft.add_learner(node_id, node, true).await?;
        }

        Ok(())
    }
}
