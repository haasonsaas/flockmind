use crate::replicator::state_machine::{HiveState, SharedState};
use crate::types::ClusterCommand;
use anyhow::Result;
use openraft::storage::{Adaptor, LogState, RaftStorage};
use openraft::{
    Entry, EntryPayload, LogId, OptionalSend, RaftLogReader, RaftSnapshotBuilder, Snapshot,
    SnapshotMeta, StorageError, StoredMembership, Vote,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::sync::Mutex;

pub type NodeIdType = u64;

openraft::declare_raft_types!(
    pub TypeConfig:
        D = ClusterCommand,
        R = (),
        Node = HiveNode,
);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HiveNode {
    pub addr: String,
    pub hostname: String,
}

impl std::fmt::Display for HiveNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.hostname, self.addr)
    }
}

pub struct HiveStorage {
    vote: Mutex<Option<Vote<NodeIdType>>>,
    log: Mutex<BTreeMap<u64, Entry<TypeConfig>>>,
    last_purged: Mutex<Option<LogId<NodeIdType>>>,
    state: SharedState,
    snapshot_idx: Mutex<u64>,
    last_applied: Mutex<Option<LogId<NodeIdType>>>,
    last_membership: Mutex<StoredMembership<NodeIdType, HiveNode>>,
}

impl HiveStorage {
    pub fn new(state: SharedState) -> Self {
        Self {
            vote: Mutex::new(None),
            log: Mutex::new(BTreeMap::new()),
            last_purged: Mutex::new(None),
            state,
            snapshot_idx: Mutex::new(0),
            last_applied: Mutex::new(None),
            last_membership: Mutex::new(StoredMembership::default()),
        }
    }

    pub fn shared_state(&self) -> &SharedState {
        &self.state
    }
}

impl RaftLogReader<TypeConfig> for HiveStorage {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<TypeConfig>>, StorageError<NodeIdType>> {
        let log = self.log.lock().unwrap();
        let entries: Vec<_> = log.range(range).map(|(_, v)| v.clone()).collect();
        Ok(entries)
    }
}

impl RaftSnapshotBuilder<TypeConfig> for HiveStorage {
    async fn build_snapshot(&mut self) -> Result<Snapshot<TypeConfig>, StorageError<NodeIdType>> {
        let hive_state = self.state.snapshot();
        let data = serde_json::to_vec(&hive_state).unwrap();

        let last_applied = self.last_applied.lock().unwrap().clone();
        let last_membership = self.last_membership.lock().unwrap().clone();

        let mut idx = self.snapshot_idx.lock().unwrap();
        *idx += 1;
        let snapshot_idx = *idx;

        let snapshot_id = format!(
            "{}-{}-{}",
            last_applied
                .map(|l| l.leader_id.to_string())
                .unwrap_or_default(),
            last_applied.map(|l| l.index).unwrap_or(0),
            snapshot_idx
        );

        let meta = SnapshotMeta {
            last_log_id: last_applied,
            last_membership,
            snapshot_id,
        };

        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(data)),
        })
    }
}

impl RaftStorage<TypeConfig> for HiveStorage {
    type LogReader = Self;
    type SnapshotBuilder = Self;

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<NodeIdType>> {
        let log = self.log.lock().unwrap();
        let last_purged = *self.last_purged.lock().unwrap();
        let last_log_id = log.iter().next_back().map(|(_, e)| e.log_id);
        Ok(LogState {
            last_purged_log_id: last_purged,
            last_log_id,
        })
    }

    async fn save_vote(&mut self, vote: &Vote<NodeIdType>) -> Result<(), StorageError<NodeIdType>> {
        *self.vote.lock().unwrap() = Some(vote.clone());
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeIdType>>, StorageError<NodeIdType>> {
        Ok(self.vote.lock().unwrap().clone())
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        HiveStorage {
            vote: Mutex::new(self.vote.lock().unwrap().clone()),
            log: Mutex::new(self.log.lock().unwrap().clone()),
            last_purged: Mutex::new(*self.last_purged.lock().unwrap()),
            state: self.state.clone(),
            snapshot_idx: Mutex::new(*self.snapshot_idx.lock().unwrap()),
            last_applied: Mutex::new(*self.last_applied.lock().unwrap()),
            last_membership: Mutex::new(self.last_membership.lock().unwrap().clone()),
        }
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<NodeIdType>>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + OptionalSend,
    {
        let mut log = self.log.lock().unwrap();
        for entry in entries {
            log.insert(entry.log_id.index, entry);
        }
        Ok(())
    }

    async fn delete_conflict_logs_since(
        &mut self,
        log_id: LogId<NodeIdType>,
    ) -> Result<(), StorageError<NodeIdType>> {
        let mut log = self.log.lock().unwrap();
        let to_remove: Vec<_> = log.range(log_id.index..).map(|(k, _)| *k).collect();
        for key in to_remove {
            log.remove(&key);
        }
        Ok(())
    }

    async fn purge_logs_upto(&mut self, log_id: LogId<NodeIdType>) -> Result<(), StorageError<NodeIdType>> {
        {
            let mut last_purged = self.last_purged.lock().unwrap();
            *last_purged = Some(log_id);
        }
        {
            let mut log = self.log.lock().unwrap();
            let to_remove: Vec<_> = log.range(..=log_id.index).map(|(k, _)| *k).collect();
            for key in to_remove {
                log.remove(&key);
            }
        }
        Ok(())
    }

    async fn last_applied_state(
        &mut self,
    ) -> Result<
        (
            Option<LogId<NodeIdType>>,
            StoredMembership<NodeIdType, HiveNode>,
        ),
        StorageError<NodeIdType>,
    > {
        let last_applied = self.last_applied.lock().unwrap().clone();
        let membership = self.last_membership.lock().unwrap().clone();
        Ok((last_applied, membership))
    }

    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry<TypeConfig>],
    ) -> Result<Vec<()>, StorageError<NodeIdType>> {
        let mut results = Vec::new();

        for entry in entries {
            *self.last_applied.lock().unwrap() = Some(entry.log_id);

            match &entry.payload {
                EntryPayload::Blank => {}
                EntryPayload::Normal(cmd) => {
                    self.state.apply(cmd);
                }
                EntryPayload::Membership(mem) => {
                    *self.last_membership.lock().unwrap() =
                        StoredMembership::new(Some(entry.log_id), mem.clone());
                }
            }
            results.push(());
        }

        Ok(results)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        HiveStorage {
            vote: Mutex::new(self.vote.lock().unwrap().clone()),
            log: Mutex::new(self.log.lock().unwrap().clone()),
            last_purged: Mutex::new(*self.last_purged.lock().unwrap()),
            state: self.state.clone(),
            snapshot_idx: Mutex::new(*self.snapshot_idx.lock().unwrap()),
            last_applied: Mutex::new(*self.last_applied.lock().unwrap()),
            last_membership: Mutex::new(self.last_membership.lock().unwrap().clone()),
        }
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<Cursor<Vec<u8>>>, StorageError<NodeIdType>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<NodeIdType, HiveNode>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<NodeIdType>> {
        let data = snapshot.into_inner();
        let hive_state: HiveState = serde_json::from_slice(&data).map_err(|e| {
            StorageError::from_io_error(
                openraft::ErrorSubject::Snapshot(Some(meta.signature())),
                openraft::ErrorVerb::Read,
                std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            )
        })?;

        self.state.restore(hive_state);
        *self.last_applied.lock().unwrap() = meta.last_log_id;
        *self.last_membership.lock().unwrap() = meta.last_membership.clone();

        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<TypeConfig>>, StorageError<NodeIdType>> {
        Ok(None)
    }
}

pub type HiveAdaptorLogStore = Adaptor<TypeConfig, HiveStorage>;
pub type HiveAdaptorStateMachine = Adaptor<TypeConfig, HiveStorage>;

pub fn create_storage(state: SharedState) -> (HiveAdaptorLogStore, HiveAdaptorStateMachine) {
    let storage = HiveStorage::new(state);
    Adaptor::new(storage)
}
