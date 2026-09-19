//! Single-owner local data plane. No Supabase URL, credential, or fallback exists here.
mod agent_bio;
#[cfg(feature = "bots")]
pub mod agent_tools;
#[cfg(feature = "bots")]
pub mod authority;
#[cfg(feature = "bots")]
pub mod bots;
pub mod enrollment;
pub mod private_code_tasks;
pub mod private_dispatch;
pub mod private_preparation;
pub mod private_readiness;
pub mod private_run;
pub mod repository;
mod transport;
pub mod tunnel;
#[cfg(feature = "bots")]
mod user_profile;
pub mod vault;
pub mod vault_curation;
pub mod vault_folder;
pub mod vault_intake;
pub mod vault_intake_folder;
pub mod vault_maintenance;
use crate::{
    capability::{Capabilities, Modality, Requirements, ToolsLevel},
    hub::*,
    ledger::Usage,
};
use chrono::Utc;
use rand::{rngs::OsRng, Rng, RngCore};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
pub use transport::{router, serve, RemoteLocalHub};
use uuid::Uuid;

type Result<T, E = HubError> = std::result::Result<T, E>;
fn rejected(s: &str) -> HubError {
    HubError::Rejected(s.into())
}
fn db_error(_: rusqlite::Error) -> HubError {
    rejected("local database operation failed")
}
fn encode<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| rejected("invalid local value"))
}
fn decode<T: for<'a> Deserialize<'a>>(s: &str) -> Result<T> {
    serde_json::from_str(s).map_err(|_| rejected("invalid stored local value"))
}
fn digest(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
fn now() -> i64 {
    Utc::now().timestamp()
}
fn check_text(s: &str, max: usize) -> Result<()> {
    if s.trim().is_empty() || s.len() > max {
        Err(rejected("invalid text length"))
    } else {
        Ok(())
    }
}
fn check_usage(u: Usage) -> Result<()> {
    if !u.compute_seconds.is_finite() || u.compute_seconds < 0.0 {
        Err(rejected("invalid usage"))
    } else {
        Ok(())
    }
}

/// Contains a secret. Deliberately does not implement Debug.
#[derive(Clone, Serialize, Deserialize)]
pub struct NodeCredentials {
    pub node_id: Uuid,
    pub raw_key: String,
}
/// Trusted local administration surface; never exposed as an HTTP API.
#[derive(Clone)]
pub struct LocalHubStore {
    db: Arc<Mutex<Connection>>,
}
/// One worker session. Clones share its identity; independent workers must connect separately.
#[derive(Clone)]
pub struct LocalHub {
    store: LocalHubStore,
    key_hash: String,
    session: Uuid,
    claim_scope: Option<Uuid>,
    run_scope: Option<Uuid>,
}

/// One schema change, with a stable identity.
///
/// The identity is the NAME, not a position. `user_version` used to be both the identity and the
/// ordering, and a single hand-allocated integer is not safe for concurrent authors: on
/// 2026-09-17 two agents each reached for "the next number" the same evening, and it only held
/// because one of them happened to look at the other's uncommitted work. The same collision is
/// already scarred into ADR-037's header, where two agents took ADR number 036 on the same day.
/// ADR-038 is the record.
///
/// Names never change once shipped -- they are what a database remembers having applied. The
/// `0001`..`0021` prefixes are frozen history from the counter era; anything added after uses a
/// timestamp prefix (`20260918-0700-…`), which sorts after them and needs no coordination with
/// anyone else's branch.
struct Migration {
    name: &'static str,
    apply: fn(&Transaction<'_>) -> Result<()>,
}

fn sql(text: &'static str) -> impl Fn(&Transaction<'_>) -> Result<()> {
    move |tx| tx.execute_batch(text).map_err(db_error)
}

macro_rules! migrations {
    ($($name:literal => $body:expr),* $(,)?) => {
        &[$(Migration { name: $name, apply: $body }),*]
    };
}

/// Applied in this order, once each, ever. Adding one means appending an entry -- no number to
/// pick, nothing to renumber, and two branches that both append merge without conflict beyond
/// the ordinary one in this list.
const MIGRATIONS: &[Migration] = migrations![
    "0001-base-schema" => |tx| sql(include_str!("schema.sql"))(tx),
    "0002-vault" => |tx| sql(include_str!("vault_schema.sql"))(tx),
    "0003-vault-folders" => |tx| sql(include_str!("vault_folder_schema.sql"))(tx),
    "0004-vault-intake" => |tx| sql(include_str!("vault_intake_schema.sql"))(tx),
    "0005-vault-curation" => |tx| sql(include_str!("vault_curation_schema.sql"))(tx),
    "0006-vault-maintenance" => |tx| sql(include_str!("vault_maintenance_schema.sql"))(tx),
    "0007-bots" => |tx| sql(include_str!("bots_schema.sql"))(tx),
    "0008-owner" => |tx| sql(include_str!("owner_schema.sql"))(tx),
    "0009-enrollment" => |tx| sql(include_str!("enrollment_schema.sql"))(tx),
    "0010-conversation-title" => |tx| {
        sql("ALTER TABLE conversations ADD COLUMN title TEXT;")(tx)
    },
    "0011-bots-causation" => |tx| sql(include_str!("bots_causation_schema.sql"))(tx),
    "0012-bots-provider" => |tx| sql(include_str!("bots_provider_schema.sql"))(tx),
    "0013-bots-room-receipts" => |tx| sql(include_str!("bots_room_receipts_schema.sql"))(tx),
    "0014-project-repositories" => |tx| {
        sql("CREATE TABLE project_repositories(project_id TEXT PRIMARY KEY REFERENCES projects(id), binding TEXT NOT NULL);")(tx)
    },
    // A claimed delivery had no expiry, only fencing, so a worker killed mid-turn left
    // `status='running'` forever and one orphan silenced its agent permanently. Nullable, and
    // only ever set while running, so pre-existing rows read as "no deadline recorded" rather
    // than "deadline long past" -- the reaper handles that NULL explicitly.
    "0015-delivery-lease-deadline" => |tx| {
        sql("ALTER TABLE agent_deliveries ADD COLUMN lease_deadline INTEGER;")(tx)
    },
    "0016-private-preparation" => |tx| sql(include_str!("private_preparation_schema.sql"))(tx),
    "0017-private-run" => |tx| sql(include_str!("private_run_schema.sql"))(tx),
    "0018-private-run-stop" => |tx| sql(include_str!("private_run_stop_schema.sql"))(tx),
    "0019-private-run-retry" => |tx| sql(include_str!("private_run_retry_schema.sql"))(tx),
    "0020-private-preparation-recovery" => |tx| {
        sql(include_str!("private_preparation_recovery_schema.sql"))(tx)
    },
    "0021-private-readiness" => |tx| sql(include_str!("private_readiness_schema.sql"))(tx),
    "0022-bots-agent-bios-and-user-profiles" => |tx| sql(include_str!("bots_profile_schema.sql"))(tx),
    "0023-bots-agent-tool-policies" => |tx| sql(include_str!("agent_tools_schema.sql"))(tx),
    "0024-bots-agent-tool-turns" => |tx| sql(include_str!("agent_tool_turns_schema.sql"))(tx),
];

/// Bring a database up to date, and refuse rather than guess when it is ahead of us.
///
/// `applied_migrations` is the source of truth. `user_version` survives as a DERIVED
/// compatibility marker -- it is set to the number of migrations this binary knows, never chosen
/// by hand -- so a binary from before this change still meets its own "newer than this worker"
/// guard and refuses, instead of opening a database it cannot understand. That guard is why the
/// 2026-09-17 vault ended up merely unreadable rather than corrupted, and it is worth keeping
/// working for old binaries that will never learn about this table.
/// Stand a current database in for an older one. `sql` undoes what the later migrations built;
/// this takes their names back off the applied list, which is what gives the engine a reason to
/// run them again. Dropping the tables alone would leave the database claiming work it no longer
/// has -- which is a corrupted database, not an older one.
#[cfg(test)]
pub(crate) fn rewind_to(db: &rusqlite::Connection, version: u32, sql: &str) {
    // Older fixtures must not retain a newer migration's receipt table.
    if version < 24 {
        db.execute_batch("DROP TABLE IF EXISTS bots_agent_tool_turns;")
            .unwrap();
    }
    db.execute_batch(sql).unwrap();
    db.execute(
        "DELETE FROM applied_migrations WHERE CAST(substr(name, 1, 4) AS INTEGER) > ?1",
        [version],
    )
    .unwrap();
    db.execute_batch(&format!("PRAGMA user_version={version}"))
        .unwrap();
}

fn migrate(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS applied_migrations(\
           name TEXT PRIMARY KEY, applied_at INTEGER NOT NULL)",
    )
    .map_err(db_error)?;

    let legacy: i64 = tx
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db_error)?;
    let known: Vec<&str> = MIGRATIONS.iter().map(|m| m.name).collect();

    let mut applied: std::collections::BTreeSet<String> = tx
        .prepare("SELECT name FROM applied_migrations")
        .map_err(db_error)?
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(db_error)?
        .collect::<std::result::Result<_, _>>()
        .map_err(db_error)?;

    // Adopting a counter-era database, which is recognisable by having no record of its own: the
    // ladder tracked progress solely in `user_version`, where `N` meant exactly "the first N steps
    // have run", so the first N names are what it has applied. Also the path the migration tests
    // take, which rewind `user_version` on a current database to stand in for an older one --
    // honouring that keeps the shortcut working without them having to know this table exists.
    //
    // Only an empty table is adopted. Once a build has written names here they are the record, and
    // a counter that disagrees with them does not get to overwrite them: this used to reseed the
    // table from `user_version`, which quietly deleted the evidence that the database had been
    // somewhere this build has never been -- exactly what the check below exists to catch.
    if applied.is_empty() && legacy >= 1 {
        if legacy as usize > known.len() {
            return Err(rejected(&format!(
                "local database is at schema {legacy} and this build only knows {}; update this \
                 machine's Hive before opening it",
                known.len()
            )));
        }
        for name in &known[..legacy as usize] {
            record_applied(tx, name)?;
            applied.insert((*name).to_string());
        }
    }

    // A database that has applied something we have never heard of is ahead of this binary. Say
    // which, because "newer than this worker" was true and unhelpful -- it could not tell anyone
    // whether they needed a newer build or were looking at a corrupted file.
    let unknown: Vec<&str> = applied
        .iter()
        .map(String::as_str)
        .filter(|n| !known.contains(n))
        .collect();
    if !unknown.is_empty() {
        return Err(rejected(&format!(
            "local database has schema changes this build does not know about ({}); update this \
             machine's Hive before opening it",
            unknown.join(", ")
        )));
    }

    for m in MIGRATIONS {
        if applied.contains(m.name) {
            continue;
        }
        (m.apply)(tx)?;
        record_applied(tx, m.name)?;
    }

    // Derived, never chosen. Two branches that each append a migration both compute the same
    // thing after they merge, which is the property the hand-allocated counter did not have.
    tx.execute_batch(&format!("PRAGMA user_version={}", MIGRATIONS.len()))
        .map_err(db_error)?;
    Ok(())
}

fn record_applied(tx: &Transaction<'_>, name: &str) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO applied_migrations(name,applied_at) VALUES(?1,?2)",
        rusqlite::params![name, now()],
    )
    .map_err(db_error)?;
    Ok(())
}

impl LocalHubStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        // Create private files before SQLite opens them (including before WAL creation).
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(path) {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(rejected("cannot create local database")),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::symlink_metadata(path)
                .map_err(|_| rejected("cannot inspect local database"))?;
            if meta.file_type().is_symlink() || meta.permissions().mode() & 0o077 != 0 {
                return Err(rejected(
                    "local database must be a private regular file (0600)",
                ));
            }
        }
        Self::from_connection(Connection::open(path).map_err(db_error)?)
    }
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory().map_err(db_error)?)
    }
    fn from_connection(mut db: Connection) -> Result<Self> {
        // Five seconds, not the 250ms this used to be. Every write goes through a BEGIN
        // IMMEDIATE transaction (see `Self::transaction`), so two concurrent writers are
        // serialised by SQLite rather than deadlocking -- but the loser only waits out this
        // timeout before SQLITE_BUSY surfaces to the member as "local database operation
        // failed". 250ms was a fail-fast value, and failing fast is the wrong behaviour for a
        // single-owner local database: a multi-statement write under real contention (room
        // creation, a card write) can exceed it, and the member gets a spurious error for an
        // operation that would have succeeded had it waited. This was reproducible: the
        // `simultaneous_retries_and_reopen_return_one_room` and
        // `separate_sqlite_connections_claim_atomically` tests failed intermittently on Linux CI
        // and fail deterministically with the timeout at 0.
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        // Serialize initialization too: reading the schema version before taking the
        // writer lock lets two first-open connections both attempt the same migration.
        // Foreign keys must be toggled outside the transaction for legacy migrations.
        let enforce_foreign_keys: bool = db
            .query_row("PRAGMA user_version", [], |r| Ok(r.get::<_, i64>(0)? >= 19))
            .map_err(db_error)?;
        db.pragma_update(None, "foreign_keys", enforce_foreign_keys)
            .map_err(db_error)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_error)?;
        migrate(&tx)?;
        tx.execute(
            "INSERT OR IGNORE INTO private_fleet_authority(id,authority_id) VALUES(1,?1)",
            [Uuid::new_v4().to_string()],
        )
        .map_err(db_error)?;
        // Revalidate source availability after every host restart.
        tx.execute("UPDATE vaults SET state='unavailable'", [])
            .map_err(db_error)?;
        if !enforce_foreign_keys
            && tx
                .prepare("PRAGMA foreign_key_check")
                .map_err(db_error)?
                .exists([])
                .map_err(db_error)?
        {
            return Err(rejected(
                "foreign key integrity check failed during initialization",
            ));
        }
        tx.commit().map_err(db_error)?;
        db.pragma_update(None, "foreign_keys", true)
            .map_err(db_error)?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
    }
    fn transaction<T>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| rejected("local database lock poisoned"))?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let result = f(&tx)?;
        tx.commit().map_err(db_error)?;
        Ok(result)
    }
    pub fn create_project(&self, title: &str, goal: &str) -> Result<Uuid> {
        check_text(title, 500)?;
        if goal.len() > 100_000 {
            return Err(rejected("goal too large"));
        }
        let id = Uuid::new_v4();
        self.transaction(|tx| {
            tx.execute(
                "INSERT INTO projects VALUES(?1,?2,?3)",
                params![id.to_string(), title, goal],
            )
            .map_err(db_error)?;
            Ok(id)
        })
    }
    pub fn add_card(&self, mut card: ClaimedCard) -> Result<Uuid> {
        self.transaction(|tx| {
            repository::apply_project_default(tx, &mut card)?;
            if card.modality == "code"
                && card
                    .required_capabilities
                    .get("repo_url")
                    .and_then(Value::as_str)
                    .is_some()
            {
                card.requires_internet = true;
            }
            validate_card(&card)?;
            tx.execute(
                "INSERT INTO cards(id,project_id,key,data) VALUES(?1,?2,?3,?4)",
                params![
                    card.id.to_string(),
                    card.project_id.to_string(),
                    card.key,
                    encode(&card)?
                ],
            )
            .map_err(db_error)?;
            Ok(card.id)
        })
    }
    pub fn configure_mcp(&self, config: &McpServerConfig, enabled: bool) -> Result<()> {
        self.transaction(|tx|{tx.execute("INSERT INTO mcp_servers VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET config=excluded.config,enabled=excluded.enabled",params![config.id.to_string(),encode(config)?,enabled]).map_err(db_error)?;Ok(())})
    }
    /// Trusted administration only: caller must have verified the Hive member identity.
    /// Never expose as an RPC or accept a member ID from an unverified client.
    /// Binding is immutable to prevent an old node key inheriting a different account.
    pub fn set_node_owner(&self, node_id: Uuid, member_id: Uuid) -> Result<()> {
        if member_id.is_nil() {
            return Err(rejected("invalid member identity"));
        }
        self.transaction(|tx| {
            let affected = tx.execute(
                "UPDATE nodes SET owner_member_id=?2 WHERE id=?1 AND (owner_member_id IS NULL OR owner_member_id=?2)",
                params![node_id.to_string(), member_id.to_string()],
            ).map_err(db_error)?;
            if affected != 1 { return Err(rejected("node not found or already bound to another account")); }
            Ok(())
        })
    }
    pub fn enroll_owner(&self, name: &str) -> Result<NodeCredentials> {
        check_text(name, 100)?;
        self.transaction(|tx| mint(tx, name))
    }

    /// Rename a computer already enrolled in this vault -- the realm name every agent on it
    /// reports.
    ///
    /// Renaming is a one-row update precisely because `AgentProfile::host_name` is derived on
    /// read: nothing else has to be rewritten, and no agent can be left introducing itself by a
    /// name the machine no longer has. Renaming three machines on 2026-09-18 left this vault's
    /// node rows saying `this machine`, `Odin.localdomain` and `Overgaard` long after the
    /// computers were Midgaard, Alfheim and Niflheim; fixing that took raw SQL against a live
    /// database, which is not a repair anyone should have to perform twice.
    ///
    /// Identity is untouched: the id, the keys and the enrollment binding all stay as they were.
    /// A name is a label, not a credential.
    pub fn rename_node(&self, node: Uuid, name: &str) -> Result<()> {
        check_text(name, 100)?;
        self.transaction(|tx| {
            let affected = tx
                .execute(
                    "UPDATE nodes SET name=?2 WHERE id=?1",
                    params![node.to_string(), name],
                )
                .map_err(db_error)?;
            if affected == 1 {
                Ok(())
            } else {
                Err(rejected("no such computer in this vault"))
            }
        })
    }
    /// One active code, five guesses total, five minutes, one successful redemption.
    pub fn pairing_code(&self) -> Result<String> {
        let code = format!("{:08}", OsRng.gen_range(0..100_000_000u32));
        self.transaction(|tx|{tx.execute("INSERT INTO pairing VALUES(1,?1,?2,0) ON CONFLICT(id) DO UPDATE SET hash=excluded.hash,expires=excluded.expires,attempts=0",params![digest(&code),now()+300]).map_err(db_error)?;Ok(code)})
    }
    pub fn redeem_pairing(&self, code: &str, name: &str) -> Result<NodeCredentials> {
        check_text(name, 100)?;
        let result = self.transaction(|tx| {
            let p: Option<(String, i64, i64)> = tx
                .query_row(
                    "SELECT hash,expires,attempts FROM pairing WHERE id=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(db_error)?;
            let Some((hash, expires, attempts)) = p else {
                return Ok(None);
            };
            if expires <= now() || attempts >= 5 {
                return Ok(None);
            };
            // Commit wrong attempts: returning an error here would roll back the limit.
            if code.len() != 8 || digest(code) != hash {
                tx.execute("UPDATE pairing SET attempts=attempts+1 WHERE id=1", [])
                    .map_err(db_error)?;
                return Ok(None);
            }
            let creds = mint(tx, name)?;
            tx.execute("DELETE FROM pairing WHERE id=1", [])
                .map_err(db_error)?;
            Ok(Some(creds))
        })?;
        result.ok_or_else(|| rejected("pairing code invalid, expired, or exhausted"))
    }
    pub fn revoke(&self, node: Uuid) -> Result<()> {
        self.transaction(|tx| {
            tx.execute(
                "UPDATE local_node_keys SET revoked=1 WHERE node_id=?1",
                [node.to_string()],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }
    pub fn connect(&self, raw_key: &str) -> Result<LocalHub> {
        self.connect_session(raw_key, Uuid::new_v4())
    }

    /// An authenticated reader session for **this machine's own** device row in this store,
    /// minting the credential on first use and reusing it forever after.
    ///
    /// A node has two identities and mixing them is a recurring trap: its Hive *account* node id
    /// (what `HubClient::whoami` returns) and its id inside a vault (issued when it enrolled).
    /// `preferred_host`, `nodes.id` and every host check are in the vault's namespace, so any
    /// process asking "which machine am I" of a vault has to ask the vault, not the account.
    ///
    /// `HIVE_VAULT_SELF_KEY` is the one answer, shared deliberately: the desktop app
    /// (`ohhive-ffi`'s `vault_open`) mints it, `coder`'s vault tools reuse it, and now so does
    /// the CLI. All three therefore resolve to the same `nodes` row. Anything that invents its
    /// own notion of self here produces an agent pinned to a computer the vault has never heard
    /// of -- which reads to the owner as "pair that computer again" and cannot be cleared by
    /// pairing anything.
    ///
    /// A stored credential that has been revoked surfaces as `BadKey` rather than being
    /// silently re-minted: re-minting would enroll a second device row for one machine, which
    /// is the same class of bug one level down.
    pub fn self_reader(&self) -> Result<LocalHub> {
        let raw_key = match crate::nodeconfig::get_extra("HIVE_VAULT_SELF_KEY") {
            Some(key) => key,
            None => {
                let credentials = self.enroll_owner("this machine")?;
                crate::nodeconfig::set("HIVE_VAULT_SELF_KEY", &credentials.raw_key)
                    .map_err(|_| rejected("couldn't save this machine's vault reader key"))?;
                credentials.raw_key
            }
        };
        self.connect(&raw_key)
    }

    /// This machine's own id in **this vault's** namespace -- see [`Self::self_reader`] for why
    /// that is not the same thing as its Hive account node id.
    pub fn self_node_id(&self) -> Result<Uuid> {
        self.self_reader()?.node_id()
    }
    pub(super) fn connect_session(&self, key: &str, session: Uuid) -> Result<LocalHub> {
        if !key.starts_with("hive_nk_") || key.len() != 56 {
            return Err(HubError::BadKey);
        }
        let hub = LocalHub {
            store: self.clone(),
            key_hash: digest(key),
            session,
            claim_scope: None,
            run_scope: None,
        };
        hub.with_node(|_, _| Ok(()))?;
        Ok(hub)
    }
    /// Owner-only local inspection. No anonymous/network read endpoint.
    pub fn inspect(&self) -> Result<Value> {
        self.transaction(|tx| {
            let mut q = tx
                .prepare("SELECT data,status,reason FROM cards ORDER BY rowid")
                .map_err(db_error)?;
            let rows = q
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                })
                .map_err(db_error)?;
            let mut cards = Vec::new();
            for row in rows {
                let (d, s, r) = row.map_err(db_error)?;
                cards.push(json!({"card":decode::<Value>(&d)?,"status":s,"reason":r}));
            }
            let mut q = tx
                .prepare("SELECT card_id,content FROM card_outputs ORDER BY rowid")
                .map_err(db_error)?;
            let rows = q
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(db_error)?;
            let mut outputs = Vec::new();
            for row in rows {
                let (id, content) = row.map_err(db_error)?;
                outputs.push(json!({"card_id":id,"content":content}));
            }
            let activity: i64 = tx
                .query_row("SELECT count(*) FROM activity", [], |r| r.get(0))
                .map_err(db_error)?;
            Ok(json!({"cards":cards,"outputs":outputs,"activity_count":activity}))
        })
    }
}
fn mint(tx: &Transaction<'_>, name: &str) -> Result<NodeCredentials> {
    let id = Uuid::new_v4();
    let mut bytes = [0u8; 24];
    OsRng.fill_bytes(&mut bytes);
    let raw_key = format!(
        "hive_nk_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    tx.execute(
        "INSERT INTO nodes(id,name) VALUES(?1,?2)",
        params![id.to_string(), name],
    )
    .map_err(db_error)?;
    tx.execute(
        "INSERT INTO local_node_keys(hash,node_id) VALUES(?1,?2)",
        params![digest(&raw_key), id.to_string()],
    )
    .map_err(db_error)?;
    Ok(NodeCredentials {
        node_id: id,
        raw_key,
    })
}
fn validate_card(c: &ClaimedCard) -> Result<()> {
    check_text(&c.key, 200)?;
    check_text(&c.title, 1000)?;
    if encode(c)?.len() > 2 * 1024 * 1024
        || c.deps.len() > 100
        || !c.required_capabilities.is_object()
    {
        return Err(rejected("invalid card"));
    }
    let _: Modality = decode(&encode(&c.modality)?)?;
    // No cloud fallback, and no community artifacts disguised as local work.
    if c.modality == "code"
        && c.required_capabilities
            .get("brain")
            .and_then(Value::as_str)
            .unwrap_or("local")
            != "local"
    {
        return Err(rejected(
            "direct cloud coding is not implemented for fully local projects",
        ));
    }
    if ["artifact_get", "artifact_get_hash", "artifact_put"]
        .iter()
        .any(|k| c.required_capabilities.get(k).is_some())
    {
        return Err(rejected(
            "community artifact tools are unavailable on local projects",
        ));
    }
    Ok(())
}
fn settle(tx: &Transaction<'_>) -> Result<()> {
    tx.execute("UPDATE cards SET status=CASE WHEN json_extract(data,'$.modality')='code' THEN 'blocked' ELSE 'ready' END, reason='lease expired; inspect workspace before retrying code' WHERE id IN (SELECT card_id FROM leases WHERE expires<=?1)",[now()]).map_err(db_error)?;
    tx.execute("DELETE FROM leases WHERE expires<=?1", [now()])
        .map_err(db_error)?;
    // Propagate child failures through all ancestor levels.
    loop {
        let n=tx.execute("UPDATE cards SET status='blocked',reason='child blocked' WHERE status='waiting_on_child' AND id IN (SELECT l.parent FROM child_links l JOIN cards c ON c.id=l.child WHERE c.status='blocked')",[]).map_err(db_error)?;
        if n == 0 {
            break;
        }
    }
    tx.execute("UPDATE cards SET status='ready' WHERE status='waiting_on_child' AND NOT EXISTS(SELECT 1 FROM child_links l JOIN cards c ON c.id=l.child WHERE l.parent=cards.id AND c.status NOT IN ('review','done'))",[]).map_err(db_error)?;
    Ok(())
}
impl LocalHub {
    /// Host-driven single-task dispatch; other ready work is never claimed.
    pub fn restricted_to_card(mut self, card: Uuid) -> Self {
        self.claim_scope = Some(card);
        self
    }

    /// This session's own device id, resolved from its key the same way every other call
    /// resolves it. For the desktop app's vault feature: a freshly created vault has no readers
    /// yet, and the machine that just created it is the obvious first grant -- this is how it
    /// finds its own id to grant without asking the owner to paste it back to themselves.
    pub fn node_id(&self) -> Result<Uuid> {
        self.with_node(|_, node| node.parse().map_err(|_| rejected("invalid node identity")))
    }
    fn with_node<T>(&self, f: impl FnOnce(&Transaction<'_>, &str) -> Result<T>) -> Result<T> {
        self.store.transaction(|tx| {
            let node: Option<String> = tx
                .query_row(
                    "SELECT node_id FROM local_node_keys WHERE hash=?1 AND revoked=0",
                    [&self.key_hash],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            f(tx, &node.ok_or(HubError::BadKey)?)
        })
    }
    fn owns(&self, tx: &Transaction<'_>, node: &str, id: Uuid) -> Result<()> {
        let owned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM leases WHERE card_id=?1 AND node_id=?2 AND session=?3 AND expires>?4)",params![id.to_string(),node,self.session.to_string(),now()],|r|r.get(0)).map_err(db_error)?;
        if owned {
            Ok(())
        } else {
            Err(rejected("no current lease for this worker session"))
        }
    }
    fn end(&self, id: Uuid, reason: &str, status: &str) -> Result<Value> {
        self.with_node(|tx, node| {
            self.owns(tx, node, id)?;
            tx.execute(
                "UPDATE cards SET status=?2,reason=?3 WHERE id=?1",
                params![id.to_string(), status, reason],
            )
            .map_err(db_error)?;
            tx.execute("DELETE FROM leases WHERE card_id=?1", [id.to_string()])
                .map_err(db_error)?;
            settle(tx)?;
            Ok(json!({"status":status}))
        })
    }
}
#[async_trait::async_trait]
impl Hub for LocalHub {
    async fn claim_card(&self) -> Result<Claim> {
        self.with_node(|tx,node|{
        settle(tx)?;
        let(caps,checked):(Option<String>,bool)=tx.query_row("SELECT caps,checked_in FROM nodes WHERE id=?1",[node],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db_error)?;
        if !checked || caps.is_none(){return Ok(Claim::NotCheckedIn)}
        let caps:Capabilities=decode(&caps.unwrap())?;
        let leased:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM leases WHERE node_id=?1)",[node],|r|r.get(0)).map_err(db_error)?;
        if leased{return Ok(Claim::AlreadyLeased)}
        let mut q=tx.prepare("SELECT data FROM cards WHERE status='ready' ORDER BY rowid").map_err(db_error)?;
        let rows=q.query_map([],|r|r.get::<_,String>(0)).map_err(db_error)?;
        for row in rows {
            let c:ClaimedCard=decode(&row.map_err(db_error)?)?;
            if self.claim_scope.is_some_and(|id| id != c.id) { continue; }
            if !private_run::allows_claim(tx, node, &c, self.run_scope, self.session)? { continue; }
            let req=Requirements{modality:Some(decode(&encode(&c.modality)?)?),model_id:c.required_capabilities.get("model_id").and_then(Value::as_str).map(str::to_owned),requires_internet:c.requires_internet,min_ram_bytes:c.required_capabilities.get("min_ram_bytes").and_then(Value::as_u64),min_vram_bytes:c.required_capabilities.get("min_vram_bytes").and_then(Value::as_u64),tools_level:if c.modality=="code" || c.required_capabilities.get("tools_level").and_then(Value::as_str)==Some("sandboxed_tools"){ToolsLevel::SandboxedTools}else{ToolsLevel::InferenceOnly}};
            if !caps.satisfies(&req){continue}
            if let Some(target)=c.required_capabilities.get("target_node_id").and_then(Value::as_str) {
                if target != node { continue; }
            }
            if let Some(server)=c.required_capabilities.get("mcp_server_id").and_then(Value::as_str) {
                if caps.tools_level != ToolsLevel::SandboxedTools { continue; }
                let enabled:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM mcp_servers WHERE id=?1 AND enabled=1)",[server],|r|r.get(0)).map_err(db_error)?;
                if !enabled { continue; }
            }
            let mut outputs=serde_json::Map::new();let mut ready=true;
            for key in &c.deps {
                let output:Option<String>=tx.query_row("SELECT o.content FROM cards d JOIN card_outputs o ON d.id=o.card_id WHERE d.project_id=?1 AND d.key=?2 AND d.status IN ('review','done')",params![c.project_id.to_string(),key],|r|r.get(0)).optional().map_err(db_error)?;
                if let Some(content)=output{outputs.insert(key.clone(),json!({"content":content}));}else{ready=false;break}
            }
            if !ready{continue}
            if c.modality == "code" && c.required_capabilities.get("coordinator").and_then(Value::as_bool)==Some(true) {
                let mut query=tx.prepare("SELECT d.id,d.key,d.status,d.data,(SELECT content FROM card_outputs WHERE card_id=d.id ORDER BY rowid DESC LIMIT 1) FROM cards d JOIN child_links l ON l.child=d.id WHERE l.parent=?1 ORDER BY d.key LIMIT 17").map_err(db_error)?;
                let rows=query.query_map([c.id.to_string()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?))).map_err(db_error)?;
                let mut children=Vec::new();
                for row in rows {
                    let(id,key,status,raw,content)=row.map_err(db_error)?;
                    let child:ClaimedCard=decode(&raw)?;
                    children.push(json!({"card_id":id,"key":key,"status":status,"modality":child.modality,
                        "checks":child.required_capabilities.get("acceptance").cloned().unwrap_or(json!([])),"content":content}));
                }
                outputs.insert("__hive_code_coordinator_v1".into(),json!({"version":1,"parent_id":c.id,"children":children}));
            }
            let project=tx.query_row("SELECT title,goal FROM projects WHERE id=?1",[c.project_id.to_string()],|r|Ok(ClaimedProject{id:c.project_id,title:r.get(0)?,goal:r.get(1)?})).map_err(db_error)?;
            let cp:Option<(u32,String,String)>=tx.query_row("SELECT step,state,usage FROM checkpoints WHERE card_id=?1",[c.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
            let checkpoint=cp.map(|(step,state,usage)|Ok(CheckpointRecord{step,blob_hash:digest(&state),state:decode(&state)?,usage:decode(&usage)?})).transpose()?;
            let ttl=if c.modality=="code"{14400}else{900};let expires=now()+ttl;
            tx.execute("INSERT INTO leases VALUES(?1,?2,?3,?4,?5)",params![c.id.to_string(),node,self.session.to_string(),expires,ttl]).map_err(db_error)?;
            tx.execute("UPDATE cards SET status='running',reason=NULL WHERE id=?1",[c.id.to_string()]).map_err(db_error)?;
            if let Some(operation) = self.run_scope {
                tx.execute("UPDATE private_runs SET state='running',session=?2 WHERE id=?1 AND state='queued'",params![operation.to_string(),self.session.to_string()]).map_err(db_error)?;
            }
            return Ok(Claim::Leased{card:c,project,dep_outputs:outputs,checkpoint,lease_expires_at:chrono::DateTime::from_timestamp(expires,0).unwrap().to_rfc3339()})
        }Ok(Claim::NothingToDo)
    })
    }
    async fn complete_card(
        &self,
        id: Uuid,
        content: &str,
        model: Option<&str>,
        usage: Usage,
    ) -> Result<Completion> {
        check_usage(usage)?;
        if content.len() > 4 * 1024 * 1024 {
            return Err(rejected("output too large"));
        }
        self.with_node(|tx,node|{
            let prev:Option<(String,String,String,Option<String>,String)>=tx.query_row("SELECT node_id,session,content,model_id,usage FROM card_outputs WHERE card_id=?1",[id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(db_error)?;
            if let Some((n,s,c,m,u))=prev {if n!=node || s!=self.session.to_string() || c!=content || m.as_deref()!=model || u!=encode(&usage)?{return Err(rejected("completion conflict"))}}
            else {self.owns(tx,node,id)?;tx.execute("INSERT INTO card_outputs VALUES(?1,?2,?3,?4,?5,?6)",params![id.to_string(),node,self.session.to_string(),content,model,encode(&usage)?]).map_err(db_error)?;
                tx.execute("UPDATE cards SET status='review' WHERE id=?1",[id.to_string()]).map_err(db_error)?;tx.execute("DELETE FROM leases WHERE card_id=?1",[id.to_string()]).map_err(db_error)?;settle(tx)?;}
            Ok(Completion{status:"review".into(),earned_honey:0.0,fund_balance:0.0,wallet_balance:0.0})
        })
    }
    async fn checkpoint(&self, id: Uuid, step: u32, state: &Value, usage: Usage) -> Result<Value> {
        check_usage(usage)?;
        let state = encode(state)?;
        if state.len() > 4 * 1024 * 1024 {
            return Err(rejected("checkpoint too large"));
        }
        self.with_node(|tx,node|{self.owns(tx,node,id)?;
            tx.execute("INSERT INTO checkpoints VALUES(?1,?2,?3,?4) ON CONFLICT(card_id) DO UPDATE SET step=excluded.step,state=excluded.state,usage=excluded.usage WHERE excluded.step>=checkpoints.step",params![id.to_string(),step,state,encode(&usage)?]).map_err(db_error)?;
            tx.execute("UPDATE leases SET expires=?2+ttl WHERE card_id=?1",params![id.to_string(),now()]).map_err(db_error)?;Ok(json!({"status":"checkpointed"}))})
    }
    async fn fail_card(&self, id: Uuid, reason: &str) -> Result<Value> {
        self.end(id, reason, "blocked")
    }
    async fn release_card(&self, id: Uuid, reason: &str) -> Result<Value> {
        // Coding sessions have no recovery record yet: never replay their side effects automatically.
        let code = self.with_node(|tx, _| {
            let s: String = tx
                .query_row(
                    "SELECT data FROM cards WHERE id=?1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            Ok(decode::<ClaimedCard>(&s)?.modality == "code")
        })?;
        self.end(id, reason, if code { "blocked" } else { "ready" })
    }
    async fn spawn_child_card(
        &self,
        parent: Uuid,
        key: &str,
        title: &str,
        modality: &str,
        inputs: &str,
        acceptance: &str,
        required: Value,
    ) -> Result<SpawnedCard> {
        self.with_node(|tx, node| {
            self.owns(tx, node, parent)?;
            let raw: String = tx
                .query_row(
                    "SELECT data FROM cards WHERE id=?1",
                    [parent.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            let p: ClaimedCard = decode(&raw)?;
            let mut required = required;
            repository::inherit_parent_repository(&p, modality, &mut required)?;
            let card = ClaimedCard {
                id: Uuid::new_v4(),
                project_id: p.project_id,
                key: key.into(),
                title: title.into(),
                modality: modality.into(),
                inputs: inputs.into(),
                acceptance: acceptance.into(),
                deps: vec![],
                requires_internet: p.requires_internet
                    || (modality == "code"
                        && required.get("repo_url").and_then(Value::as_str).is_some()),
                required_capabilities: required,
            };
            validate_card(&card)?;
            let existing: Option<String> = tx
                .query_row(
                    "SELECT data FROM cards WHERE project_id=?1 AND key=?2",
                    params![p.project_id.to_string(), key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            let child = if let Some(raw) = existing {
                let old: ClaimedCard = decode(&raw)?;
                let linked: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM child_links WHERE parent=?1 AND child=?2)",
                        params![parent.to_string(), old.id.to_string()],
                        |r| r.get(0),
                    )
                    .map_err(db_error)?;
                if !linked
                    || old.inputs != inputs
                    || old.title != title
                    || old.modality != modality
                    || old.acceptance != acceptance
                    || old.required_capabilities != card.required_capabilities
                {
                    return Err(rejected("child key conflict"));
                }
                old
            } else {
                tx.execute(
                    "INSERT INTO cards(id,project_id,key,data) VALUES(?1,?2,?3,?4)",
                    params![
                        card.id.to_string(),
                        card.project_id.to_string(),
                        key,
                        encode(&card)?
                    ],
                )
                .map_err(db_error)?;
                tx.execute(
                    "INSERT INTO child_links VALUES(?1,?2)",
                    params![parent.to_string(), card.id.to_string()],
                )
                .map_err(db_error)?;
                card
            };
            Ok(SpawnedCard {
                card_id: child.id,
                key: child.key,
                project_id: child.project_id,
                requires_internet: child.requires_internet,
            })
        })
    }
    async fn wait_on_child(&self, id: Uuid, child: Uuid) -> Result<Value> {
        self.with_node(|tx, node| {
            self.owns(tx, node, id)?;
            let linked: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM child_links WHERE parent=?1 AND child=?2)",
                    params![id.to_string(), child.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            if !linked {
                return Err(rejected("not this parent's child"));
            }
            let raw: String = tx
                .query_row(
                    "SELECT data FROM cards WHERE id=?1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            let mut p: ClaimedCard = decode(&raw)?;
            let key: String = tx
                .query_row(
                    "SELECT key FROM cards WHERE id=?1",
                    [child.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            if !p.deps.contains(&key) {
                p.deps.push(key)
            }
            tx.execute(
                "UPDATE cards SET status='waiting_on_child',data=?2 WHERE id=?1",
                params![id.to_string(), encode(&p)?],
            )
            .map_err(db_error)?;
            tx.execute("DELETE FROM leases WHERE card_id=?1", [id.to_string()])
                .map_err(db_error)?;
            settle(tx)?;
            Ok(json!({"status":"waiting_on_child"}))
        })
    }
    async fn mcp_server_config(&self, id: Uuid) -> Result<McpServerConfig> {
        self.with_node(|tx, node| {
            let tools: Option<String> = tx
                .query_row("SELECT caps FROM nodes WHERE id=?1", [node], |r| r.get(0))
                .map_err(db_error)?;
            if tools
                .map(|c| decode::<Capabilities>(&c))
                .transpose()?
                .map(|c| c.tools_level)
                != Some(ToolsLevel::SandboxedTools)
            {
                return Err(rejected("MCP requires enabled tools"));
            }
            let s: Option<String> = tx
                .query_row(
                    "SELECT config FROM mcp_servers WHERE id=?1 AND enabled=1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            decode(&s.ok_or_else(|| rejected("MCP server not configured or disabled"))?)
        })
    }
    async fn check_in(&self, caps: &Capabilities, _region: Option<&str>) -> Result<Value> {
        self.with_node(|tx, node| {
            tx.execute(
                "UPDATE nodes SET caps=?2,checked_in=1 WHERE id=?1",
                params![node, encode(caps)?],
            )
            .map_err(db_error)?;
            Ok(json!({"presence":"checked_in"}))
        })
    }
    async fn heartbeat(&self, _rtt: Option<u64>) -> Result<(String, u64)> {
        self.with_node(|tx, node| {
            settle(tx)?;
            tx.execute(
                "UPDATE leases SET expires=?3+ttl WHERE node_id=?1 AND session=?2",
                params![node, self.session.to_string(), now()],
            )
            .map_err(db_error)?;
            Ok((Utc::now().to_rfc3339(), 0))
        })
    }
    async fn check_out(&self) -> Result<String> {
        self.with_node(|tx, node| {
            tx.execute("UPDATE nodes SET checked_in=0 WHERE id=?1", [node])
                .map_err(db_error)?;
            let leased: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM leases WHERE node_id=?1)",
                    [node],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            Ok(if leased { "draining" } else { "checked_out" }.into())
        })
    }
    async fn get_schedule(&self) -> Result<Option<Value>> {
        self.with_node(|_, _| Ok(None))
    }
    async fn post_activity(&self, kind: &str, body: &str, payload: Value) -> Result<()> {
        if body.len() > 65536 || encode(&payload)?.len() > 262144 {
            return Err(rejected("activity too large"));
        }
        self.with_node(|tx, node| {
            tx.execute(
                "INSERT INTO activity(node_id,kind,body,payload,created) VALUES(?1,?2,?3,?4,?5)",
                params![node, kind, body, encode(&payload)?, now()],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
