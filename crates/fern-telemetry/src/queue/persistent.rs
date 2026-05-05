//! redb-backed `EventQueue` that survives process restart.
//!
//! Single table, key = monotonic `u64` (FIFO order = ascending key
//! order), value = `serde_json` bytes of [`PersistedRecord`].
//! Events are bounded by capacity (oldest dropped past the cap) and
//! by age (events older than `max_age` are dropped during the next
//! drain or push).
//!
//! Per-event retry metadata (`attempts`, `next_attempt_at_unix_ms`)
//! is reserved in the schema for a future scheduled-retry feature
//! but isn't currently consulted on drain — adapters retry the same
//! batch using their own backoff state.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fern_core::telemetry::OwnedEvent;
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};

use super::EventQueue;

const EVENTS: TableDefinition<u64, &[u8]> = TableDefinition::new("events");
const NEXT_ID: TableDefinition<&str, u64> = TableDefinition::new("next_id");
const NEXT_ID_KEY: &str = "next";

/// Wire shape persisted on disk. Versioned by tag for future schema
/// migrations (just bump the variant; serde_json's untagged fallback
/// makes additive changes painless).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRecord {
    /// Schema version of *this record*. Currently always 1.
    #[serde(default = "default_version")]
    record_version: u32,
    event: OwnedEvent,
    enqueued_at_unix_ms: u64,
    /// Reserved for future per-event scheduled-retry — currently
    /// always 0. Adapters drive backoff from their own state today.
    #[serde(default)]
    attempts: u32,
    /// Reserved for future per-event scheduled-retry — currently
    /// always 0.
    #[serde(default)]
    next_attempt_at_unix_ms: u64,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, thiserror::Error)]
pub enum PersistentQueueError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("redb error: {0}")]
    Database(#[from] redb::Error),
    #[error("serialize error: {0}")]
    Serialize(#[from] serde_json::Error),
}

impl From<redb::DatabaseError> for PersistentQueueError {
    fn from(e: redb::DatabaseError) -> Self {
        Self::Database(e.into())
    }
}

impl From<redb::TransactionError> for PersistentQueueError {
    fn from(e: redb::TransactionError) -> Self {
        Self::Database(e.into())
    }
}

impl From<redb::TableError> for PersistentQueueError {
    fn from(e: redb::TableError) -> Self {
        Self::Database(e.into())
    }
}

impl From<redb::StorageError> for PersistentQueueError {
    fn from(e: redb::StorageError) -> Self {
        Self::Database(e.into())
    }
}

impl From<redb::CommitError> for PersistentQueueError {
    fn from(e: redb::CommitError) -> Self {
        Self::Database(e.into())
    }
}

/// redb-backed event queue. `Send + Sync` — internal `Mutex` wraps
/// the `Database` handle so adapter UI-thread pushes and worker-
/// thread drains don't conflict.
pub struct PersistentEventQueue {
    db: Mutex<Database>,
    path: PathBuf,
    capacity: usize,
    max_age: Duration,
}

impl PersistentEventQueue {
    /// Open or create the queue file at `path`. Parent directory is
    /// created if missing.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, PersistentQueueError> {
        Self::open_with(path, 10_000, Duration::from_secs(60 * 60 * 24 * 7))
    }

    /// Like [`open`](Self::open) but with explicit `capacity` (events
    /// past which the oldest is dropped) and `max_age` (events older
    /// than this are dropped on next push or drain).
    pub fn open_with(
        path: impl Into<PathBuf>,
        capacity: usize,
        max_age: Duration,
    ) -> Result<Self, PersistentQueueError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Database::create(&path)?;
        // Initialize the tables so first-read transactions don't
        // observe TableDoesNotExist.
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(EVENTS)?;
            let _ = write_txn.open_table(NEXT_ID)?;
        }
        write_txn.commit()?;
        Ok(Self {
            db: Mutex::new(db),
            path,
            capacity: capacity.max(1),
            max_age,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn now_unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Drop entries older than `max_age` and the oldest entries past
    /// `capacity`. Called from `push` so the queue self-prunes.
    fn evict_locked(
        write_txn: &redb::WriteTransaction,
        capacity: usize,
        max_age: Duration,
        now_ms: u64,
    ) -> Result<(), PersistentQueueError> {
        let mut events = write_txn.open_table(EVENTS)?;
        let max_age_ms = max_age.as_millis() as u64;
        // Collect keys to delete to avoid holding an iterator while
        // mutating the table.
        let mut to_delete: Vec<u64> = Vec::new();
        for entry in events.iter()? {
            let (k, v) = entry?;
            let key = k.value();
            let bytes = v.value();
            if let Ok(record) = serde_json::from_slice::<PersistedRecord>(bytes) {
                let age_ms = now_ms.saturating_sub(record.enqueued_at_unix_ms);
                if age_ms >= max_age_ms {
                    to_delete.push(key);
                }
            } else {
                // Corrupt entry — drop it.
                to_delete.push(key);
            }
        }
        // Capacity: ensure post-insert size won't exceed `capacity`.
        // We're called *before* the insert, so we need at most
        // `capacity - 1` events after this eviction completes.
        let len_after_age = events.len()? as usize - to_delete.len();
        if len_after_age + 1 > capacity {
            let drop_n = len_after_age + 1 - capacity;
            let surviving_keys: Vec<u64> = events
                .iter()?
                .filter_map(|e| e.ok().map(|(k, _)| k.value()))
                .filter(|k| !to_delete.contains(k))
                .collect();
            for key in surviving_keys.into_iter().take(drop_n) {
                to_delete.push(key);
            }
        }
        for key in to_delete {
            events.remove(key)?;
        }
        Ok(())
    }

    fn next_id_locked(write_txn: &redb::WriteTransaction) -> Result<u64, PersistentQueueError> {
        let mut next_id_table = write_txn.open_table(NEXT_ID)?;
        let current = next_id_table
            .get(NEXT_ID_KEY)?
            .map(|g| g.value())
            .unwrap_or(0);
        let next = current.wrapping_add(1);
        next_id_table.insert(NEXT_ID_KEY, next)?;
        Ok(next)
    }
}

impl EventQueue for PersistentEventQueue {
    fn push(&self, event: OwnedEvent) {
        let db = self.db.lock().expect("queue db mutex poisoned");
        if let Err(e) = (|| -> Result<(), PersistentQueueError> {
            let now_ms = Self::now_unix_ms();
            let write_txn = db.begin_write()?;
            // Evict before insert to keep the queue bounded.
            Self::evict_locked(&write_txn, self.capacity, self.max_age, now_ms)?;
            let id = Self::next_id_locked(&write_txn)?;
            {
                let record = PersistedRecord {
                    record_version: 1,
                    event,
                    enqueued_at_unix_ms: now_ms,
                    attempts: 0,
                    next_attempt_at_unix_ms: 0,
                };
                let bytes = serde_json::to_vec(&record)?;
                let mut events = write_txn.open_table(EVENTS)?;
                events.insert(id, bytes.as_slice())?;
            }
            write_txn.commit()?;
            Ok(())
        })() {
            // Telemetry must never panic the host application — log
            // and drop. Real adapters can poll a separate health
            // signal if they care.
            eprintln!("fern-telemetry: persistent queue push failed: {e}");
        }
    }

    fn drain_batch(&self, n: usize) -> Vec<OwnedEvent> {
        let db = self.db.lock().expect("queue db mutex poisoned");
        match (|| -> Result<Vec<OwnedEvent>, PersistentQueueError> {
            let write_txn = db.begin_write()?;
            let mut taken: Vec<(u64, OwnedEvent)> = Vec::new();
            {
                let events = write_txn.open_table(EVENTS)?;
                for entry in events.iter()? {
                    if taken.len() >= n {
                        break;
                    }
                    let (k, v) = entry?;
                    let key = k.value();
                    let bytes = v.value();
                    match serde_json::from_slice::<PersistedRecord>(bytes) {
                        Ok(rec) => taken.push((key, rec.event)),
                        Err(_) => taken.push((
                            key,
                            OwnedEvent {
                                // Synthesize a placeholder for corrupt
                                // entries so the caller sees an event
                                // count but the bad row gets removed.
                                name: "telemetry.corrupt".into(),
                                category: fern_core::telemetry::EventCategory::Custom,
                                timestamp: SystemTime::UNIX_EPOCH,
                                install_id: None,
                                session_id: String::new(),
                                schema_version: 0,
                                props: vec![],
                            },
                        )),
                    }
                }
            }
            {
                let mut events = write_txn.open_table(EVENTS)?;
                for (k, _) in &taken {
                    events.remove(*k)?;
                }
            }
            write_txn.commit()?;
            Ok(taken.into_iter().map(|(_, e)| e).collect())
        })() {
            Ok(events) => events,
            Err(e) => {
                eprintln!("fern-telemetry: persistent queue drain failed: {e}");
                Vec::new()
            }
        }
    }

    fn len(&self) -> usize {
        let db = self.db.lock().expect("queue db mutex poisoned");
        (|| -> Result<usize, PersistentQueueError> {
            let read_txn = db.begin_read()?;
            let events = read_txn.open_table(EVENTS)?;
            Ok(events.len()? as usize)
        })()
        .unwrap_or_default()
    }

    fn discard_all(&self) {
        let db = self.db.lock().expect("queue db mutex poisoned");
        if let Err(e) = (|| -> Result<(), PersistentQueueError> {
            let write_txn = db.begin_write()?;
            {
                let mut events = write_txn.open_table(EVENTS)?;
                let keys: Vec<u64> = events
                    .iter()?
                    .filter_map(|e| e.ok().map(|(k, _)| k.value()))
                    .collect();
                for key in keys {
                    events.remove(key)?;
                }
            }
            write_txn.commit()?;
            Ok(())
        })() {
            eprintln!("fern-telemetry: persistent queue discard failed: {e}");
        }
    }

    fn peek_recent(&self, n: usize) -> Vec<OwnedEvent> {
        let db = self.db.lock().expect("queue db mutex poisoned");
        (|| -> Result<Vec<OwnedEvent>, PersistentQueueError> {
            let read_txn = db.begin_read()?;
            let events = read_txn.open_table(EVENTS)?;
            let mut out = Vec::with_capacity(n);
            // Iterate in reverse order (newest first) by collecting
            // and reversing — redb doesn't expose a reverse-iterator
            // builder generically, but the table is small (<10k).
            let mut all: Vec<(u64, Vec<u8>)> = Vec::new();
            for entry in events.iter()? {
                let (k, v) = entry?;
                all.push((k.value(), v.value().to_vec()));
            }
            for (_, bytes) in all.into_iter().rev().take(n) {
                if let Ok(rec) = serde_json::from_slice::<PersistedRecord>(&bytes) {
                    out.push(rec.event);
                }
            }
            Ok(out)
        })()
        .unwrap_or_default()
    }
}

impl std::fmt::Debug for PersistentEventQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistentEventQueue")
            .field("path", &self.path)
            .field("capacity", &self.capacity)
            .field("max_age", &self.max_age)
            .field("len", &self.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::telemetry::EventCategory;
    use tempfile::tempdir;

    fn ev(name: &str) -> OwnedEvent {
        OwnedEvent {
            name: name.to_string(),
            category: EventCategory::Intent,
            timestamp: SystemTime::UNIX_EPOCH,
            install_id: None,
            session_id: "test".into(),
            schema_version: 1,
            props: vec![],
        }
    }

    #[test]
    fn open_creates_file_and_starts_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("queue.redb");
        let q = PersistentEventQueue::open(&path).unwrap();
        assert!(path.exists());
        assert_eq!(q.len(), 0);
        assert!(q.is_empty());
    }

    #[test]
    fn push_then_drain_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("q.redb");
        let q = PersistentEventQueue::open(&path).unwrap();
        q.push(ev("a"));
        q.push(ev("b"));
        q.push(ev("c"));
        assert_eq!(q.len(), 3);
        let batch = q.drain_batch(10);
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0].name, "a");
        assert_eq!(batch[2].name, "c");
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn events_survive_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("survive.redb");
        {
            let q = PersistentEventQueue::open(&path).unwrap();
            q.push(ev("event_1"));
            q.push(ev("event_2"));
            assert_eq!(q.len(), 2);
            // Drop the queue without draining.
        }
        // Reopen and verify the events are still there.
        let q = PersistentEventQueue::open(&path).unwrap();
        assert_eq!(q.len(), 2);
        let batch = q.drain_batch(10);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].name, "event_1");
        assert_eq!(batch[1].name, "event_2");
    }

    #[test]
    fn capacity_evicts_oldest_on_push() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cap.redb");
        let q = PersistentEventQueue::open_with(&path, 3, Duration::from_secs(3600)).unwrap();
        q.push(ev("a"));
        q.push(ev("b"));
        q.push(ev("c"));
        q.push(ev("d"));
        q.push(ev("e"));
        assert_eq!(q.len(), 3);
        let batch = q.drain_batch(10);
        let names: Vec<String> = batch.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["c".to_string(), "d".into(), "e".into()]);
    }

    #[test]
    fn discard_all_empties() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("discard.redb");
        let q = PersistentEventQueue::open(&path).unwrap();
        q.push(ev("a"));
        q.push(ev("b"));
        q.discard_all();
        assert!(q.is_empty());
    }

    #[test]
    fn discard_persists_across_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("discard2.redb");
        {
            let q = PersistentEventQueue::open(&path).unwrap();
            q.push(ev("a"));
            q.discard_all();
        }
        let q = PersistentEventQueue::open(&path).unwrap();
        assert!(q.is_empty());
    }

    #[test]
    fn peek_recent_returns_newest_first() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("peek.redb");
        let q = PersistentEventQueue::open(&path).unwrap();
        q.push(ev("a"));
        q.push(ev("b"));
        q.push(ev("c"));
        let recent = q.peek_recent(2);
        assert_eq!(recent[0].name, "c");
        assert_eq!(recent[1].name, "b");
    }

    #[test]
    fn drain_batch_takes_only_up_to_n() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("batch.redb");
        let q = PersistentEventQueue::open(&path).unwrap();
        for name in ["a", "b", "c", "d", "e"] {
            q.push(ev(name));
        }
        let batch = q.drain_batch(2);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].name, "a");
        assert_eq!(batch[1].name, "b");
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn fifo_order_preserved_across_reopen() {
        // Insert, reopen, drain — order must match insertion order.
        let dir = tempdir().unwrap();
        let path = dir.path().join("fifo.redb");
        {
            let q = PersistentEventQueue::open(&path).unwrap();
            for name in ["first", "second", "third", "fourth"] {
                q.push(ev(name));
            }
        }
        let q = PersistentEventQueue::open(&path).unwrap();
        let batch = q.drain_batch(10);
        let names: Vec<String> = batch.iter().map(|e| e.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                "first".to_string(),
                "second".into(),
                "third".into(),
                "fourth".into()
            ]
        );
    }
}
