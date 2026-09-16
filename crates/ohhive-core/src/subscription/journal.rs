//! Host-local subscription turn journal. This is a trusted local API, not a remote
//! authorization boundary. Callers must resolve the owner/account/workspace first.
//! Separate database: opening this never migrates the shared LocalHub database.
//! Persist dispatch intent BEFORE the provider call. A false `mark_dispatched`
//! means "do not send". An expired writer's dispatched turns become unknown and
//! block new work until the current writer reconciles the provider's result.
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("Subscription journal storage failed")]
    Storage,
    #[error("Subscription session binding differs from the stored binding")]
    Binding,
    #[error("Subscription session has an active writer")]
    Busy,
    #[error("Subscription writer is stale or expired")]
    Stale,
    #[error("Subscription turn requires reconciliation before continuing")]
    Uncertain,
    #[error("Subscription operation conflicts with a recorded operation")]
    Conflict,
    #[error("Invalid subscription journal input")]
    Invalid,
}
type Result<T> = std::result::Result<T, JournalError>;
fn db(_: rusqlite::Error) -> JournalError {
    JournalError::Storage
}
fn timestamp() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| JournalError::Storage)?
        .as_secs() as i64)
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Chatgpt,
    Copilot,
    Grok,
}

/// Opaque identity references only; never credentials, transcript or environment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Binding {
    pub session: Uuid,
    pub owner: Uuid,
    pub host: Uuid,
    pub agent: Uuid,
    pub conversation: Uuid,
    pub account: Uuid,
    pub provider: Provider,
    pub workspace: Uuid,
    pub policy_revision: String,
}
#[derive(Debug, Clone)]
pub struct Lease {
    session: Uuid,
    writer: Uuid,
    generation: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRecord {
    pub state: String,
    pub provider_turn: Option<String>,
    /// Reference to an already persisted result; not its content.
    pub receipt: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTurn {
    pub operation: Uuid,
    pub input_hash: String,
    pub record: TurnRecord,
}
pub struct Journal {
    connection: Connection,
}
impl Journal {
    pub fn open(path: &Path) -> Result<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(JournalError::Storage),
        }
        let meta = std::fs::symlink_metadata(path).map_err(|_| JournalError::Storage)?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(JournalError::Invalid);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if meta.permissions().mode() & 0o077 != 0 || meta.nlink() != 1 {
                return Err(JournalError::Invalid);
            }
        }
        let connection = Connection::open(path).map_err(db)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db)?;
        connection
            .execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(db)?;
        let mut journal = Self { connection };
        let tx = journal
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        // application_id prevents accidentally initializing this schema in a LocalHub DB.
        let app: i64 = tx
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .map_err(db)?;
        let version: i64 = tx
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db)?;
        if app == 0 && version == 0 {
            let tables: i64 = tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |r| r.get(0)).map_err(db)?;
            if tables != 0 {
                return Err(JournalError::Invalid);
            }
            tx.execute_batch("CREATE TABLE sessions(id TEXT PRIMARY KEY, binding TEXT NOT NULL, writer TEXT, generation INTEGER NOT NULL DEFAULT 0, expires INTEGER NOT NULL DEFAULT 0);
                CREATE TABLE turns(session TEXT NOT NULL REFERENCES sessions(id), operation TEXT NOT NULL, input_hash TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('prepared','dispatched','delivery_unknown','completed','failed')), provider_turn TEXT, receipt TEXT, PRIMARY KEY(session,operation));
                CREATE INDEX turns_pending ON turns(session,state);
                PRAGMA application_id=1213416010; PRAGMA user_version=1;").map_err(db)?;
        } else if app != 1213416010 || version != 1 {
            return Err(JournalError::Invalid);
        }
        tx.commit().map_err(db)?;
        Ok(journal)
    }
    pub fn acquire(&mut self, binding: &Binding, writer: Uuid, seconds: u32) -> Result<Lease> {
        self.acquire_at(binding, writer, seconds, timestamp()?)
    }
    fn acquire_at(
        &mut self,
        binding: &Binding,
        writer: Uuid,
        seconds: u32,
        now: i64,
    ) -> Result<Lease> {
        if !(1..=300).contains(&seconds)
            || binding.policy_revision.is_empty()
            || binding.policy_revision.len() > 256
        {
            return Err(JournalError::Invalid);
        }
        let encoded = serde_json::to_string(binding).map_err(|_| JournalError::Invalid)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        tx.execute(
            "INSERT OR IGNORE INTO sessions(id,binding) VALUES(?1,?2)",
            params![binding.session.to_string(), encoded],
        )
        .map_err(db)?;
        let (stored, generation, expires): (String, i64, i64) = tx
            .query_row(
                "SELECT binding,generation,expires FROM sessions WHERE id=?1",
                [binding.session.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(db)?;
        if stored != encoded {
            return Err(JournalError::Binding);
        }
        if expires > now {
            return Err(JournalError::Busy);
        }
        let generation = generation.checked_add(1).ok_or(JournalError::Invalid)?;
        tx.execute(
            "UPDATE turns SET state='delivery_unknown' WHERE session=?1 AND state='dispatched'",
            [binding.session.to_string()],
        )
        .map_err(db)?;
        tx.execute(
            "UPDATE sessions SET writer=?2,generation=?3,expires=?4 WHERE id=?1",
            params![
                binding.session.to_string(),
                writer.to_string(),
                generation,
                now + i64::from(seconds)
            ],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(Lease {
            session: binding.session,
            writer,
            generation,
        })
    }
    fn fence(tx: &Transaction<'_>, lease: &Lease, now: i64) -> Result<()> {
        let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND writer=?2 AND generation=?3 AND expires>?4)", params![lease.session.to_string(),lease.writer.to_string(),lease.generation,now], |r| r.get(0)).map_err(db)?;
        if valid {
            Ok(())
        } else {
            Err(JournalError::Stale)
        }
    }
    pub fn renew(&mut self, lease: &Lease, seconds: u32) -> Result<()> {
        if !(1..=300).contains(&seconds) {
            return Err(JournalError::Invalid);
        }
        let now = timestamp()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        Self::fence(&tx, lease, now)?;
        tx.execute(
            "UPDATE sessions SET expires=?2 WHERE id=?1",
            params![lease.session.to_string(), now + i64::from(seconds)],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
    }
    pub fn prepare(
        &mut self,
        lease: &Lease,
        operation: Uuid,
        input_hash: &str,
    ) -> Result<TurnRecord> {
        if input_hash.len() != 64 || !input_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(JournalError::Invalid);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        Self::fence(&tx, lease, timestamp()?)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT input_hash FROM turns WHERE session=?1 AND operation=?2",
                params![lease.session.to_string(), operation.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        if let Some(hash) = existing {
            if hash != input_hash {
                return Err(JournalError::Conflict);
            }
        } else {
            let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM turns WHERE session=?1 AND state IN ('prepared','dispatched','delivery_unknown'))", [lease.session.to_string()], |r| r.get(0)).map_err(db)?;
            if pending {
                return Err(JournalError::Uncertain);
            }
            tx.execute(
                "INSERT INTO turns(session,operation,input_hash,state) VALUES(?1,?2,?3,'prepared')",
                params![lease.session.to_string(), operation.to_string(), input_hash],
            )
            .map_err(db)?;
        }
        let record = Self::read(&tx, lease, operation)?;
        tx.commit().map_err(db)?;
        Ok(record)
    }
    fn read(tx: &Transaction<'_>, lease: &Lease, operation: Uuid) -> Result<TurnRecord> {
        tx.query_row(
            "SELECT state,provider_turn,receipt FROM turns WHERE session=?1 AND operation=?2",
            params![lease.session.to_string(), operation.to_string()],
            |r| {
                Ok(TurnRecord {
                    state: r.get(0)?,
                    provider_turn: r.get(1)?,
                    receipt: r.get(2)?,
                })
            },
        )
        .map_err(db)
    }
    /// Only a true return authorizes the caller to perform the first provider send.
    pub fn mark_dispatched(&mut self, lease: &Lease, operation: Uuid) -> Result<bool> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        Self::fence(&tx, lease, timestamp()?)?;
        let changed = tx.execute("UPDATE turns SET state='dispatched' WHERE session=?1 AND operation=?2 AND state='prepared'",params![lease.session.to_string(),operation.to_string()]).map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(changed == 1)
    }
    /// Recovery enumeration; contains identifiers/hashes only, not prompt contents.
    pub fn pending(&mut self, lease: &Lease) -> Result<Vec<PendingTurn>> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        Self::fence(&tx, lease, timestamp()?)?;
        let result = {
            let mut query = tx.prepare("SELECT operation,input_hash,state,provider_turn,receipt FROM turns WHERE session=?1 AND state IN ('prepared','dispatched','delivery_unknown') LIMIT 2").map_err(db)?;
            let rows = query
                .query_map([lease.session.to_string()], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        TurnRecord {
                            state: r.get(2)?,
                            provider_turn: r.get(3)?,
                            receipt: r.get(4)?,
                        },
                    ))
                })
                .map_err(db)?;
            let mut result = Vec::new();
            for row in rows {
                let (operation, input_hash, record) = row.map_err(db)?;
                result.push(PendingTurn {
                    operation: Uuid::parse_str(&operation).map_err(|_| JournalError::Storage)?,
                    input_hash,
                    record,
                });
            }
            if result.len() > 1 {
                return Err(JournalError::Storage);
            }
            result
        };
        tx.commit().map_err(db)?;
        Ok(result)
    }
    /// Persist an acknowledged provider ID before waiting for the final result.
    pub fn acknowledge(
        &mut self,
        lease: &Lease,
        operation: Uuid,
        provider_turn: &str,
    ) -> Result<()> {
        if provider_turn.is_empty() || provider_turn.len() > 256 {
            return Err(JournalError::Invalid);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        Self::fence(&tx, lease, timestamp()?)?;
        let old = Self::read(&tx, lease, operation)?;
        if !matches!(old.state.as_str(), "dispatched" | "delivery_unknown")
            || old
                .provider_turn
                .as_deref()
                .is_some_and(|id| id != provider_turn)
        {
            return Err(JournalError::Conflict);
        }
        tx.execute(
            "UPDATE turns SET provider_turn=?3 WHERE session=?1 AND operation=?2",
            params![
                lease.session.to_string(),
                operation.to_string(),
                provider_turn
            ],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
    }
    /// Stop writing immediately. Unfinished sends remain unknown, never requeued.
    pub fn release(&mut self, lease: &Lease) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        Self::fence(&tx, lease, timestamp()?)?;
        tx.execute(
            "UPDATE turns SET state='delivery_unknown' WHERE session=?1 AND state='dispatched'",
            [lease.session.to_string()],
        )
        .map_err(db)?;
        tx.execute(
            "UPDATE sessions SET expires=0,writer=NULL WHERE id=?1",
            [lease.session.to_string()],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
    }
    /// An uncertain send may be completed only from reconciled provider evidence.
    /// Does not permit re-sending it or modifying an already committed terminal receipt.
    pub fn finish(
        &mut self,
        lease: &Lease,
        operation: Uuid,
        provider_turn: &str,
        receipt: &str,
        succeeded: bool,
    ) -> Result<()> {
        if provider_turn.is_empty()
            || provider_turn.len() > 256
            || receipt.is_empty()
            || receipt.len() > 256
        {
            return Err(JournalError::Invalid);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        Self::fence(&tx, lease, timestamp()?)?;
        let previous = Self::read(&tx, lease, operation)?;
        if previous
            .provider_turn
            .as_deref()
            .is_some_and(|id| id != provider_turn)
        {
            return Err(JournalError::Conflict);
        }
        let state = if succeeded { "completed" } else { "failed" };
        if previous.state == state
            && previous.provider_turn.as_deref() == Some(provider_turn)
            && previous.receipt.as_deref() == Some(receipt)
        {
            return Ok(());
        }
        if !matches!(previous.state.as_str(), "dispatched" | "delivery_unknown") {
            return Err(JournalError::Conflict);
        }
        tx.execute("UPDATE turns SET state=?3,provider_turn=?4,receipt=?5 WHERE session=?1 AND operation=?2",params![lease.session.to_string(),operation.to_string(),state,provider_turn,receipt]).map_err(db)?;
        tx.commit().map_err(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "hive-subscription-journal-{}.sqlite",
                Uuid::new_v4()
            )))
        }
        fn open(&self) -> Journal {
            Journal::open(&self.0).unwrap()
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn binding() -> Binding {
        Binding {
            session: Uuid::new_v4(),
            owner: Uuid::new_v4(),
            host: Uuid::new_v4(),
            agent: Uuid::new_v4(),
            conversation: Uuid::new_v4(),
            account: Uuid::new_v4(),
            provider: Provider::Copilot,
            workspace: Uuid::new_v4(),
            policy_revision: "1".into(),
        }
    }
    fn expire(j: &Journal) {
        j.connection
            .execute("UPDATE sessions SET expires=0", [])
            .unwrap();
    }
    const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    #[test]
    fn acknowledged_turn_survives_release_and_cannot_change_provider_id() {
        let temp = Temp::new();
        let mut j = temp.open();
        let b = binding();
        let op = Uuid::new_v4();
        let lease = j.acquire(&b, Uuid::new_v4(), 60).unwrap();
        j.prepare(&lease, op, HASH).unwrap();
        j.mark_dispatched(&lease, op).unwrap();
        j.acknowledge(&lease, op, "provider-turn-1").unwrap();
        j.release(&lease).unwrap();
        assert!(matches!(j.pending(&lease), Err(JournalError::Stale)));
        drop(j);
        let mut j = temp.open();
        let next = j.acquire(&b, Uuid::new_v4(), 60).unwrap();
        let pending = j.pending(&next).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].operation, op);
        assert_eq!(
            pending[0].record.provider_turn.as_deref(),
            Some("provider-turn-1")
        );
        assert_eq!(pending[0].record.state, "delivery_unknown");
        assert!(matches!(
            j.finish(&next, op, "other-turn", "receipt", true),
            Err(JournalError::Conflict)
        ));
        j.finish(&next, op, "provider-turn-1", "receipt", true)
            .unwrap();
        assert!(j.pending(&next).unwrap().is_empty());
    }
    #[test]
    fn crash_after_dispatch_cannot_replay_and_fences_old_writer() {
        let temp = Temp::new();
        let mut first = temp.open();
        let b = binding();
        let op = Uuid::new_v4();
        let old = first.acquire(&b, Uuid::new_v4(), 60).unwrap();
        first.prepare(&old, op, HASH).unwrap();
        assert!(first.mark_dispatched(&old, op).unwrap());
        assert!(!first.mark_dispatched(&old, op).unwrap());
        expire(&first);
        drop(first);
        let mut reopened = temp.open();
        let current = reopened.acquire(&b, Uuid::new_v4(), 60).unwrap();
        assert_eq!(
            reopened.prepare(&current, op, HASH).unwrap().state,
            "delivery_unknown"
        );
        assert!(!reopened.mark_dispatched(&current, op).unwrap());
        assert!(matches!(
            reopened.prepare(&current, Uuid::new_v4(), HASH),
            Err(JournalError::Uncertain)
        ));
        assert!(matches!(
            reopened.finish(&old, op, "turn", "receipt", true),
            Err(JournalError::Stale)
        ));
        reopened
            .finish(&current, op, "turn", "receipt", true)
            .unwrap();
        reopened
            .finish(&current, op, "turn", "receipt", true)
            .unwrap();
        assert!(matches!(
            reopened.finish(&current, op, "turn", "changed", true),
            Err(JournalError::Conflict)
        ));
        assert_eq!(
            reopened.prepare(&current, op, HASH).unwrap().state,
            "completed"
        );
        assert!(!reopened.mark_dispatched(&current, op).unwrap());
        reopened.prepare(&current, Uuid::new_v4(), HASH).unwrap();
    }
    #[test]
    fn crash_before_dispatch_leaves_intent_sendable_once() {
        let temp = Temp::new();
        let mut j = temp.open();
        let b = binding();
        let op = Uuid::new_v4();
        let lease = j.acquire(&b, Uuid::new_v4(), 60).unwrap();
        j.prepare(&lease, op, HASH).unwrap();
        expire(&j);
        drop(j);
        let mut j = temp.open();
        let lease = j.acquire(&b, Uuid::new_v4(), 60).unwrap();
        assert_eq!(j.prepare(&lease, op, HASH).unwrap().state, "prepared");
        assert!(j.mark_dispatched(&lease, op).unwrap());
        assert!(!j.mark_dispatched(&lease, op).unwrap());
    }
    #[test]
    fn binding_and_payload_are_immutable() {
        let temp = Temp::new();
        let mut j = temp.open();
        let b = binding();
        let lease = j.acquire(&b, Uuid::new_v4(), 60).unwrap();
        let op = Uuid::new_v4();
        j.prepare(&lease, op, HASH).unwrap();
        assert!(matches!(
            j.prepare(&lease, op, &"b".repeat(64)),
            Err(JournalError::Conflict)
        ));
        expire(&j);
        let mut changed = b.clone();
        changed.account = Uuid::new_v4();
        assert!(matches!(
            j.acquire(&changed, Uuid::new_v4(), 60),
            Err(JournalError::Binding)
        ));
        changed = b.clone();
        changed.owner = Uuid::new_v4();
        assert!(matches!(
            j.acquire(&changed, Uuid::new_v4(), 60),
            Err(JournalError::Binding)
        ));
        changed = b;
        changed.policy_revision = "2".into();
        assert!(matches!(
            j.acquire(&changed, Uuid::new_v4(), 60),
            Err(JournalError::Binding)
        ));
    }
    #[test]
    fn independent_connections_have_one_writer() {
        let temp = Temp::new();
        drop(temp.open());
        let b = binding();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let path = temp.0.clone();
                let binding = b.clone();
                let gate = barrier.clone();
                std::thread::spawn(move || {
                    let mut j = Journal::open(&path).unwrap();
                    gate.wait();
                    j.acquire(&binding, Uuid::new_v4(), 60)
                })
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(JournalError::Busy)))
                .count(),
            1
        );
    }
    #[test]
    fn expired_writer_cannot_renew_or_dispatch() {
        let temp = Temp::new();
        let mut j = temp.open();
        let b = binding();
        let lease = j.acquire(&b, Uuid::new_v4(), 60).unwrap();
        let op = Uuid::new_v4();
        j.prepare(&lease, op, HASH).unwrap();
        j.renew(&lease, 60).unwrap();
        expire(&j);
        assert!(matches!(j.renew(&lease, 60), Err(JournalError::Stale)));
        assert!(matches!(
            j.mark_dispatched(&lease, op),
            Err(JournalError::Stale)
        ));
    }
    #[test]
    fn refuses_unrelated_database_and_unknown_schema() {
        let temp = Temp::new();
        let j = temp.open();
        j.connection
            .execute_batch("PRAGMA user_version=2;")
            .unwrap();
        drop(j);
        assert!(Journal::open(&temp.0).is_err());
        let other = Temp::new();
        let mut file = std::fs::OpenOptions::new();
        file.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            file.mode(0o600);
        }
        file.open(&other.0).unwrap();
        let c = Connection::open(&other.0).unwrap();
        c.execute_batch("CREATE TABLE unrelated(id TEXT);").unwrap();
        drop(c);
        assert!(Journal::open(&other.0).is_err());
    }
}
