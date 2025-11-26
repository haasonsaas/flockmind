use crate::replicator::state_machine::{HiveState, SharedState};
use crate::types::ClusterCommand;
use anyhow::Result;
use openraft::storage::{Adaptor, LogState, RaftStorage};
use openraft::{
    Entry, EntryPayload, LogId, OptionalSend, RaftLogReader, RaftSnapshotBuilder, Snapshot,
    SnapshotMeta, StorageError, StoredMembership, Vote,
};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::path::Path;
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

const KEY_VOTE: &[u8] = b"vote";
const KEY_LAST_PURGED: &[u8] = b"last_purged";
const KEY_LAST_APPLIED: &[u8] = b"last_applied";
const KEY_MEMBERSHIP: &[u8] = b"membership";
const KEY_SNAPSHOT_IDX: &[u8] = b"snapshot_idx";
const KEY_STATE_SNAPSHOT: &[u8] = b"state_snapshot";

pub struct SledStorage {
    db: sled::Db,
    log_tree: sled::Tree,
    meta_tree: sled::Tree,
    state: SharedState,
    snapshot_idx: Mutex<u64>,
}

impl SledStorage {
    pub fn new<P: AsRef<Path>>(path: P, state: SharedState) -> Result<Self> {
        let db = sled::open(path)?;
        let log_tree = db.open_tree("raft_log")?;
        let meta_tree = db.open_tree("raft_meta")?;

        let snapshot_idx = meta_tree
            .get(KEY_SNAPSHOT_IDX)?
            .map(|v| bincode::deserialize(&v).unwrap_or(0))
            .unwrap_or(0);

        if let Some(state_data) = meta_tree.get(KEY_STATE_SNAPSHOT)? {
            if let Ok(hive_state) = serde_json::from_slice::<HiveState>(&state_data) {
                state.restore(hive_state);
                tracing::info!("Restored state from snapshot");
            }
        }

        Ok(Self {
            db,
            log_tree,
            meta_tree,
            state,
            snapshot_idx: Mutex::new(snapshot_idx),
        })
    }

    fn log_key(index: u64) -> [u8; 8] {
        index.to_be_bytes()
    }

    fn get_vote(&self) -> Option<Vote<NodeIdType>> {
        self.meta_tree
            .get(KEY_VOTE)
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize(&v).ok())
    }

    fn set_vote(&self, vote: &Vote<NodeIdType>) -> Result<(), sled::Error> {
        let data = bincode::serialize(vote).unwrap();
        self.meta_tree.insert(KEY_VOTE, data)?;
        self.meta_tree.flush()?;
        Ok(())
    }

    fn get_last_purged(&self) -> Option<LogId<NodeIdType>> {
        self.meta_tree
            .get(KEY_LAST_PURGED)
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize(&v).ok())
    }

    fn set_last_purged(&self, log_id: &LogId<NodeIdType>) -> Result<(), sled::Error> {
        let data = bincode::serialize(log_id).unwrap();
        self.meta_tree.insert(KEY_LAST_PURGED, data)?;
        Ok(())
    }

    fn get_last_applied(&self) -> Option<LogId<NodeIdType>> {
        self.meta_tree
            .get(KEY_LAST_APPLIED)
            .ok()
            .flatten()
            .and_then(|v| bincode::deserialize(&v).ok())
    }

    fn set_last_applied(&self, log_id: &LogId<NodeIdType>) -> Result<(), sled::Error> {
        let data = bincode::serialize(log_id).unwrap();
        self.meta_tree.insert(KEY_LAST_APPLIED, data)?;
        Ok(())
    }

    fn get_membership(&self) -> StoredMembership<NodeIdType, HiveNode> {
        self.meta_tree
            .get(KEY_MEMBERSHIP)
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default()
    }

    fn set_membership(&self, membership: &StoredMembership<NodeIdType, HiveNode>) -> Result<(), sled::Error> {
        let data = serde_json::to_vec(membership).unwrap();
        self.meta_tree.insert(KEY_MEMBERSHIP, data)?;
        Ok(())
    }

    fn save_state_snapshot(&self) -> Result<(), sled::Error> {
        let hive_state = self.state.snapshot();
        let data = serde_json::to_vec(&hive_state).unwrap();
        self.meta_tree.insert(KEY_STATE_SNAPSHOT, data)?;
        self.meta_tree.flush()?;
        Ok(())
    }

    pub fn shared_state(&self) -> &SharedState {
        &self.state
    }
}

impl RaftLogReader<TypeConfig> for SledStorage {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<TypeConfig>>, StorageError<NodeIdType>> {
        let start = match range.start_bound() {
            std::ops::Bound::Included(&s) => s,
            std::ops::Bound::Excluded(&s) => s + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&e) => Some(e + 1),
            std::ops::Bound::Excluded(&e) => Some(e),
            std::ops::Bound::Unbounded => None,
        };

        let mut entries = Vec::new();
        for item in self.log_tree.range(Self::log_key(start)..) {
            let (key, value) = item.map_err(|e| {
                StorageError::from_io_error(
                    openraft::ErrorSubject::Logs,
                    openraft::ErrorVerb::Read,
                    std::io::Error::new(std::io::ErrorKind::Other, e),
                )
            })?;

            let index = u64::from_be_bytes(key.as_ref().try_into().unwrap());
            if let Some(e) = end {
                if index >= e {
                    break;
                }
            }

            let entry: Entry<TypeConfig> = serde_json::from_slice(&value).map_err(|e| {
                StorageError::from_io_error(
                    openraft::ErrorSubject::Logs,
                    openraft::ErrorVerb::Read,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, e),
                )
            })?;
            entries.push(entry);
        }

        Ok(entries)
    }
}

impl RaftSnapshotBuilder<TypeConfig> for SledStorage {
    async fn build_snapshot(&mut self) -> Result<Snapshot<TypeConfig>, StorageError<NodeIdType>> {
        let hive_state = self.state.snapshot();
        let data = serde_json::to_vec(&hive_state).unwrap();

        let last_applied = self.get_last_applied();
        let last_membership = self.get_membership();

        let mut idx = self.snapshot_idx.lock().unwrap();
        *idx += 1;
        let snapshot_idx = *idx;

        let _ = self.meta_tree.insert(
            KEY_SNAPSHOT_IDX,
            bincode::serialize(&snapshot_idx).unwrap(),
        );

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

impl RaftStorage<TypeConfig> for SledStorage {
    type LogReader = Self;
    type SnapshotBuilder = Self;

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<NodeIdType>> {
        let last_purged = self.get_last_purged();

        let last_log_id = self
            .log_tree
            .last()
            .map_err(|e| {
                StorageError::from_io_error(
                    openraft::ErrorSubject::Logs,
                    openraft::ErrorVerb::Read,
                    std::io::Error::new(std::io::ErrorKind::Other, e),
                )
            })?
            .and_then(|(_, v)| serde_json::from_slice::<Entry<TypeConfig>>(&v).ok())
            .map(|e| e.log_id);

        Ok(LogState {
            last_purged_log_id: last_purged,
            last_log_id,
        })
    }

    async fn save_vote(&mut self, vote: &Vote<NodeIdType>) -> Result<(), StorageError<NodeIdType>> {
        self.set_vote(vote).map_err(|e| {
            StorageError::from_io_error(
                openraft::ErrorSubject::Vote,
                openraft::ErrorVerb::Write,
                std::io::Error::new(std::io::ErrorKind::Other, e),
            )
        })
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeIdType>>, StorageError<NodeIdType>> {
        Ok(self.get_vote())
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        SledStorage {
            db: self.db.clone(),
            log_tree: self.log_tree.clone(),
            meta_tree: self.meta_tree.clone(),
            state: self.state.clone(),
            snapshot_idx: Mutex::new(*self.snapshot_idx.lock().unwrap()),
        }
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<NodeIdType>>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + OptionalSend,
    {
        for entry in entries {
            let key = Self::log_key(entry.log_id.index);
            let value = serde_json::to_vec(&entry).unwrap();
            self.log_tree.insert(key, value).map_err(|e| {
                StorageError::from_io_error(
                    openraft::ErrorSubject::Logs,
                    openraft::ErrorVerb::Write,
                    std::io::Error::new(std::io::ErrorKind::Other, e),
                )
            })?;
        }
        self.log_tree.flush().map_err(|e| {
            StorageError::from_io_error(
                openraft::ErrorSubject::Logs,
                openraft::ErrorVerb::Write,
                std::io::Error::new(std::io::ErrorKind::Other, e),
            )
        })?;
        Ok(())
    }

    async fn delete_conflict_logs_since(
        &mut self,
        log_id: LogId<NodeIdType>,
    ) -> Result<(), StorageError<NodeIdType>> {
        let keys_to_remove: Vec<_> = self
            .log_tree
            .range(Self::log_key(log_id.index)..)
            .filter_map(|r| r.ok().map(|(k, _)| k))
            .collect();

        for key in keys_to_remove {
            self.log_tree.remove(key).map_err(|e| {
                StorageError::from_io_error(
                    openraft::ErrorSubject::Logs,
                    openraft::ErrorVerb::Write,
                    std::io::Error::new(std::io::ErrorKind::Other, e),
                )
            })?;
        }
        Ok(())
    }

    async fn purge_logs_upto(
        &mut self,
        log_id: LogId<NodeIdType>,
    ) -> Result<(), StorageError<NodeIdType>> {
        self.set_last_purged(&log_id).map_err(|e| {
            StorageError::from_io_error(
                openraft::ErrorSubject::Logs,
                openraft::ErrorVerb::Write,
                std::io::Error::new(std::io::ErrorKind::Other, e),
            )
        })?;

        let keys_to_remove: Vec<_> = self
            .log_tree
            .range(..=Self::log_key(log_id.index))
            .filter_map(|r| r.ok().map(|(k, _)| k))
            .collect();

        for key in keys_to_remove {
            self.log_tree.remove(key).map_err(|e| {
                StorageError::from_io_error(
                    openraft::ErrorSubject::Logs,
                    openraft::ErrorVerb::Write,
                    std::io::Error::new(std::io::ErrorKind::Other, e),
                )
            })?;
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
        Ok((self.get_last_applied(), self.get_membership()))
    }

    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry<TypeConfig>],
    ) -> Result<Vec<()>, StorageError<NodeIdType>> {
        let mut results = Vec::new();

        for entry in entries {
            self.set_last_applied(&entry.log_id).map_err(|e| {
                StorageError::from_io_error(
                    openraft::ErrorSubject::StateMachine,
                    openraft::ErrorVerb::Write,
                    std::io::Error::new(std::io::ErrorKind::Other, e),
                )
            })?;

            match &entry.payload {
                EntryPayload::Blank => {}
                EntryPayload::Normal(cmd) => {
                    self.state.apply(cmd);
                }
                EntryPayload::Membership(mem) => {
                    let membership = StoredMembership::new(Some(entry.log_id), mem.clone());
                    self.set_membership(&membership).map_err(|e| {
                        StorageError::from_io_error(
                            openraft::ErrorSubject::StateMachine,
                            openraft::ErrorVerb::Write,
                            std::io::Error::new(std::io::ErrorKind::Other, e),
                        )
                    })?;
                }
            }
            results.push(());
        }

        self.save_state_snapshot().map_err(|e| {
            StorageError::from_io_error(
                openraft::ErrorSubject::StateMachine,
                openraft::ErrorVerb::Write,
                std::io::Error::new(std::io::ErrorKind::Other, e),
            )
        })?;

        Ok(results)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        SledStorage {
            db: self.db.clone(),
            log_tree: self.log_tree.clone(),
            meta_tree: self.meta_tree.clone(),
            state: self.state.clone(),
            snapshot_idx: Mutex::new(*self.snapshot_idx.lock().unwrap()),
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

        if let Some(log_id) = meta.last_log_id {
            self.set_last_applied(&log_id).map_err(|e| {
                StorageError::from_io_error(
                    openraft::ErrorSubject::StateMachine,
                    openraft::ErrorVerb::Write,
                    std::io::Error::new(std::io::ErrorKind::Other, e),
                )
            })?;
        }

        self.set_membership(&meta.last_membership).map_err(|e| {
            StorageError::from_io_error(
                openraft::ErrorSubject::StateMachine,
                openraft::ErrorVerb::Write,
                std::io::Error::new(std::io::ErrorKind::Other, e),
            )
        })?;

        self.save_state_snapshot().map_err(|e| {
            StorageError::from_io_error(
                openraft::ErrorSubject::StateMachine,
                openraft::ErrorVerb::Write,
                std::io::Error::new(std::io::ErrorKind::Other, e),
            )
        })?;

        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<TypeConfig>>, StorageError<NodeIdType>> {
        Ok(None)
    }
}

pub type SledAdaptorLogStore = Adaptor<TypeConfig, SledStorage>;
pub type SledAdaptorStateMachine = Adaptor<TypeConfig, SledStorage>;

pub fn create_storage<P: AsRef<Path>>(
    path: P,
    state: SharedState,
) -> Result<(SledAdaptorLogStore, SledAdaptorStateMachine)> {
    let storage = SledStorage::new(path, state)?;
    Ok(Adaptor::new(storage))
}
