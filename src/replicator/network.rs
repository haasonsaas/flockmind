use crate::replicator::storage::{HiveNode, NodeIdType, TypeConfig};
use openraft::error::{InstallSnapshotError, NetworkError, RPCError, RaftError};
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct HiveNetworkFactory {
    connections: Arc<RwLock<HashMap<NodeIdType, String>>>,
}

impl HiveNetworkFactory {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register_node(&self, node_id: NodeIdType, addr: String) {
        self.connections.write().unwrap().insert(node_id, addr);
    }

    #[allow(dead_code)]
    pub fn get_addr(&self, node_id: NodeIdType) -> Option<String> {
        self.connections.read().unwrap().get(&node_id).cloned()
    }
}

impl Default for HiveNetworkFactory {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HiveNetwork {
    #[allow(dead_code)]
    target: NodeIdType,
    target_addr: String,
    client: reqwest::Client,
}

impl HiveNetwork {
    pub fn new(target: NodeIdType, target_addr: String) -> Self {
        Self {
            target,
            target_addr,
            client: reqwest::Client::new(),
        }
    }

    async fn send_rpc<Req, Resp, E>(
        &self,
        path: &str,
        req: &Req,
    ) -> Result<Resp, RPCError<NodeIdType, HiveNode, RaftError<NodeIdType, E>>>
    where
        Req: serde::Serialize,
        Resp: serde::de::DeserializeOwned,
        E: std::error::Error,
    {
        let url = format!("http://{}/raft/{}", self.target_addr, path);

        let response = self
            .client
            .post(&url)
            .json(req)
            .send()
            .await
            .map_err(|e| RPCError::Network(NetworkError::new(&e)))?;

        if !response.status().is_success() {
            return Err(RPCError::Network(NetworkError::new(&std::io::Error::other(
                format!("HTTP error: {}", response.status()),
            ))));
        }

        response
            .json()
            .await
            .map_err(|e| RPCError::Network(NetworkError::new(&e)))
    }
}

impl RaftNetworkFactory<TypeConfig> for HiveNetworkFactory {
    type Network = HiveNetwork;

    async fn new_client(&mut self, target: NodeIdType, node: &HiveNode) -> Self::Network {
        HiveNetwork::new(target, node.addr.clone())
    }
}

impl RaftNetwork<TypeConfig> for HiveNetwork {
    async fn append_entries(
        &mut self,
        req: AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<NodeIdType>, RPCError<NodeIdType, HiveNode, RaftError<NodeIdType>>>
    {
        self.send_rpc("append_entries", &req).await
    }

    async fn install_snapshot(
        &mut self,
        req: InstallSnapshotRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<NodeIdType>,
        RPCError<NodeIdType, HiveNode, RaftError<NodeIdType, InstallSnapshotError>>,
    > {
        self.send_rpc("install_snapshot", &req).await
    }

    async fn vote(
        &mut self,
        req: VoteRequest<NodeIdType>,
        _option: RPCOption,
    ) -> Result<VoteResponse<NodeIdType>, RPCError<NodeIdType, HiveNode, RaftError<NodeIdType>>> {
        self.send_rpc("vote", &req).await
    }
}
