//! Single-owner local data plane. No Supabase URL, credential, or fallback exists here.
mod transport;
pub mod tunnel;
pub mod vault;
pub mod vault_curation;
pub mod vault_maintenance;
pub mod vault_folder;
pub mod vault_intake;
pub mod vault_intake_folder;
#[cfg(feature = "bots")]
pub mod bots;
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
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db_error)?;
        if version > 8 {
            return Err(rejected("local database schema is newer than this worker"));
        }
        db.busy_timeout(std::time::Duration::from_millis(250))
            .map_err(db_error)?;
        db.pragma_update(None, "foreign_keys", true)
            .map_err(db_error)?;
        let tx = db.transaction().map_err(db_error)?;
        if version == 0 {
            tx.execute_batch(include_str!("schema.sql"))
                .map_err(db_error)?;
        }
        if version < 2 {
            tx.execute_batch(include_str!("vault_schema.sql"))
                .map_err(db_error)?;
        }
        if version < 3 {
            tx.execute_batch(include_str!("vault_folder_schema.sql"))
                .map_err(db_error)?;
        }
        if version < 4 {
            tx.execute_batch(include_str!("vault_intake_schema.sql")).map_err(db_error)?;
        }
        if version < 5 {
            tx.execute_batch(include_str!("vault_curation_schema.sql")).map_err(db_error)?;
        }
        if version < 6 {
            tx.execute_batch(include_str!("vault_maintenance_schema.sql")).map_err(db_error)?;
        }
        if version < 7 {
            tx.execute_batch(include_str!("bots_schema.sql")).map_err(db_error)?;
        }
        if version < 8 {
            tx.execute_batch(include_str!("owner_schema.sql")).map_err(db_error)?;
        }
        // Revalidate source availability after every host restart.
        tx.execute("UPDATE vaults SET state='unavailable'", [])
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
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
        self.transaction(|tx| {
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
        if member_id.is_nil() { return Err(rejected("invalid member identity")); }
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
    pub(super) fn connect_session(&self, key: &str, session: Uuid) -> Result<LocalHub> {
        if !key.starts_with("hive_nk_") || key.len() != 56 {
            return Err(HubError::BadKey);
        }
        let hub = LocalHub {
            store: self.clone(),
            key_hash: digest(key),
            session,
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
    /// This session's own device id, resolved from its key the same way every other call
    /// resolves it. For the desktop app's vault feature: a freshly created vault has no readers
    /// yet, and the machine that just created it is the obvious first grant -- this is how it
    /// finds its own id to grant without asking the owner to paste it back to themselves.
    pub fn node_id(&self) -> Result<Uuid> {
        self.with_node(|_, node| {
            node.parse().map_err(|_| rejected("invalid node identity"))
        })
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
            let req=Requirements{modality:Some(decode(&encode(&c.modality)?)?),model_id:c.required_capabilities.get("model_id").and_then(Value::as_str).map(str::to_owned),requires_internet:c.requires_internet,min_ram_bytes:c.required_capabilities.get("min_ram_bytes").and_then(Value::as_u64),min_vram_bytes:c.required_capabilities.get("min_vram_bytes").and_then(Value::as_u64),tools_level:if c.modality=="code" || c.required_capabilities.get("tools_level").and_then(Value::as_str)==Some("sandboxed_tools"){ToolsLevel::SandboxedTools}else{ToolsLevel::InferenceOnly},..Default::default()};
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
            let project=tx.query_row("SELECT title,goal FROM projects WHERE id=?1",[c.project_id.to_string()],|r|Ok(ClaimedProject{id:c.project_id,title:r.get(0)?,goal:r.get(1)?})).map_err(db_error)?;
            let cp:Option<(u32,String,String)>=tx.query_row("SELECT step,state,usage FROM checkpoints WHERE card_id=?1",[c.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
            let checkpoint=cp.map(|(step,state,usage)|Ok(CheckpointRecord{step,blob_hash:digest(&state),state:decode(&state)?,usage:decode(&usage)?})).transpose()?;
            let ttl=if c.modality=="code"{14400}else{900};let expires=now()+ttl;
            tx.execute("INSERT INTO leases VALUES(?1,?2,?3,?4,?5)",params![c.id.to_string(),node,self.session.to_string(),expires,ttl]).map_err(db_error)?;
            tx.execute("UPDATE cards SET status='running',reason=NULL WHERE id=?1",[c.id.to_string()]).map_err(db_error)?;
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
