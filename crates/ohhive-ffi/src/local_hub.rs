//! FFI wrapper for the Private Fleet vault (ADR-028, "central-query v1" -- Jack, 2026-09-14: a
//! second-brain knowledge library, scoped to the member's own Private Fleet, not the whole Hive).
//! Sif built and tested the Rust storage/search/HTTP-reader layer (`hive_core::local_hub::vault`,
//! `local_hub::transport`) but flagged, in her build report, that there is **zero** UniFFI wiring
//! from that layer to this Swift app -- this file is that wiring, same division of labor as
//! `coder.rs`/`worker.rs` (Loki's) vs. the Edge Function/web UI (Sif's) for ADR-024.
//!
//! **Scope of this first pass: this machine only.** A member opens a vault store on this Mac
//! (`vault_open`), creates vaults, and adds/removes Markdown notes by hand (`vault_add_note`/
//! `vault_remove_note`) -- no folder-watching yet (`vault_folder.rs` exists in core but isn't
//! wired here; that's a real, separate next increment, not forgotten). Reading goes through a
//! real `LocalHub` session for this machine, auto-granted on every vault this machine creates --
//! there is no product reason to make the owner paste their own machine's device id to grant
//! itself the vault it just made.
//!
//! **2026-09-27: reading a vault from a *second* Private Fleet machine is wired up now** --
//! Jack's correction that day, verbatim: "no matter which machine I'm on I should be seeing the
//! same data. This is our Private Fleet Library." `VaultState` gained a `remote` slot
//! (`RemoteLocalHub`, built from the credentials `hive hub pair` saves once it also records the
//! hub's own origin -- see `PairedHubCredentials`), and every read method
//! (`vault_list`/`vault_search`/`vault_list_documents`/`vault_read`) checks it first, falling
//! back to the local in-process `LocalHub` reader for a never-paired or hub-hosting machine. This
//! was exactly the "small, additive follow-up (swap in a `RemoteLocalHub` branch)" this doc
//! comment used to say the plumbing (Sif's central-query v1 report) had been built for.
//! `vault_create` (and the other host-local admin calls below it) are unchanged: only whoever can
//! open the hub machine's own vault file can administer it, so a paired client machine is
//! refused with a clear message rather than silently writing to its own empty local vault.
//!
//! All state here is synchronous (SQLite on the local disk, no network), so unlike most of this
//! crate's methods these are plain `pub fn`, not `async fn` -- no reason to touch `RUNTIME`.

use crate::RUNTIME;
use crate::{HiveError, HiveNode};
use hive_core::local_hub::vault::{
    VaultDocument as CoreDoc, VaultHit as CoreHit, VaultInfo as CoreInfo,
};
use hive_core::local_hub::vault_intake::IntakeReceipt as CoreReceipt;
use hive_core::local_hub::vault_intake_folder::IntakeCandidate as CoreCandidate;
use hive_core::local_hub::vault_maintenance::{
    MaintenancePolicy as CoreMaintenancePolicy, MaintenanceResult as CoreMaintenanceResult,
    MaintenanceStatus as CoreMaintenanceStatus,
};
use hive_core::local_hub::{LocalHub, LocalHubStore, RemoteLocalHub};
use hive_core::nodeconfig;
use std::sync::Mutex;
use uuid::Uuid;

/// Paired computer access to a collection stored on this Mac.
#[derive(uniffi::Record, Clone)]
pub struct VaultComputerAccess {
    pub node_id: String,
    pub name: String,
    pub allowed: bool,
    pub active: bool,
}

#[derive(uniffi::Record, Clone)]
pub struct VaultInfo {
    pub id: String,
    pub name: String,
    /// "ready" | "unavailable" -- mirrors the core's `VaultInfo.state` verbatim rather than
    /// collapsing it to a bool, since a folder-backed vault (future work) can be temporarily
    /// unavailable without being gone.
    pub state: String,
}
impl From<CoreInfo> for VaultInfo {
    fn from(v: CoreInfo) -> Self {
        Self {
            id: v.id.to_string(),
            name: v.name,
            state: v.state,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct VaultDocument {
    pub id: String,
    pub vault_id: String,
    pub path: String,
    pub revision: String,
    pub title: String,
    pub content: String,
}
impl From<CoreDoc> for VaultDocument {
    fn from(d: CoreDoc) -> Self {
        Self {
            id: d.id.to_string(),
            vault_id: d.vault_id.to_string(),
            path: d.path,
            revision: d.revision,
            title: d.title,
            content: d.content,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct VaultHit {
    pub id: String,
    pub path: String,
    pub revision: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
}
impl From<CoreHit> for VaultHit {
    fn from(h: CoreHit) -> Self {
        Self {
            id: h.id.to_string(),
            path: h.path,
            revision: h.revision,
            title: h.title,
            snippet: h.snippet,
            score: h.score,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct VaultHostStatus {
    pub store_path: String,
    pub vaults: Vec<VaultInfo>,
}

/// One `.md` file offered to a member for approval into a managed vault
/// (`hive_core::local_hub::vault_intake_folder`).
#[derive(uniffi::Record, Clone)]
pub struct IntakeCandidate {
    pub relative_path: String,
    pub title: String,
    pub size: u64,
}
impl From<CoreCandidate> for IntakeCandidate {
    fn from(c: CoreCandidate) -> Self {
        Self {
            relative_path: c.relative_path,
            title: c.title,
            size: c.size,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct IntakeReceipt {
    pub document_id: String,
    pub revision: Option<String>,
    pub unchanged: bool,
}
impl From<CoreReceipt> for IntakeReceipt {
    fn from(r: CoreReceipt) -> Self {
        Self {
            document_id: r.document_id.to_string(),
            revision: r.revision,
            unchanged: r.unchanged,
        }
    }
}

fn store_path() -> std::path::PathBuf {
    nodeconfig::path().with_file_name("vault-host.sqlite3")
}
fn parse_uuid(s: &str, what: &str) -> Result<Uuid, HiveError> {
    Uuid::parse_str(s).map_err(|_| HiveError::Failed(format!("not a valid {what}")))
}
/// 2026-09-15, Loki: wiring for `vault_maintenance.rs` (Sif's staleness/duplicate scanning +
/// snapshot retention, built and tested but never started by any host, and never exposed here).
/// Deliberately trimmed vs. the core `MaintenanceResult` -- no candidate id/duplicate-pair lists,
/// just counts -- this is a status surface ("last run found 3 stale notes"), not a review queue;
/// reviewing individual stale/duplicate items already goes through `vault_curation`'s own tools.
#[derive(uniffi::Record, Clone)]
pub struct VaultMaintenancePolicy {
    pub enabled: bool,
    pub interval_seconds: u32,
    pub stale_after_days: u32,
    /// None retains every snapshot. Some(days) opts into discarding aged redundant copies.
    pub redundant_snapshot_days: Option<u32>,
    pub archive_quota_bytes: i64,
}
impl From<VaultMaintenancePolicy> for CoreMaintenancePolicy {
    fn from(p: VaultMaintenancePolicy) -> Self {
        Self {
            enabled: p.enabled,
            interval_seconds: p.interval_seconds,
            stale_after_days: p.stale_after_days,
            redundant_snapshot_days: p.redundant_snapshot_days,
            archive_quota_bytes: p.archive_quota_bytes,
        }
    }
}
impl From<CoreMaintenancePolicy> for VaultMaintenancePolicy {
    fn from(p: CoreMaintenancePolicy) -> Self {
        Self {
            enabled: p.enabled,
            interval_seconds: p.interval_seconds,
            stale_after_days: p.stale_after_days,
            redundant_snapshot_days: p.redundant_snapshot_days,
            archive_quota_bytes: p.archive_quota_bytes,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct VaultMaintenanceResult {
    pub finished_ms: i64,
    pub outcome: String,
    pub notes: u32,
    pub stale: u32,
    pub duplicates: u32,
    pub duplicate_scan_complete: bool,
    pub snapshots_expired: u32,
    pub findings_truncated: bool,
    pub archive_bytes: i64,
    pub over_quota: bool,
}
impl From<CoreMaintenanceResult> for VaultMaintenanceResult {
    fn from(r: CoreMaintenanceResult) -> Self {
        Self {
            finished_ms: r.finished_ms,
            outcome: r.outcome,
            notes: r.notes as u32,
            stale: r.stale as u32,
            duplicates: r.duplicates as u32,
            duplicate_scan_complete: r.duplicate_scan_complete,
            snapshots_expired: r.snapshots_expired as u32,
            findings_truncated: r.findings_truncated,
            archive_bytes: r.archive_bytes,
            over_quota: r.over_quota,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct VaultMaintenanceStatus {
    pub policy: VaultMaintenancePolicy,
    pub next_due_ms: i64,
    pub running: bool,
    pub last_result: Option<VaultMaintenanceResult>,
}
impl From<CoreMaintenanceStatus> for VaultMaintenanceStatus {
    fn from(s: CoreMaintenanceStatus) -> Self {
        Self {
            policy: s.policy.into(),
            next_due_ms: s.next_due_ms,
            running: s.running,
            last_result: s.last_result.map(Into::into),
        }
    }
}

fn not_open() -> HiveError {
    HiveError::Failed("open the vault first (vault_open)".into())
}
fn poisoned() -> HiveError {
    HiveError::Failed("vault state lock poisoned".into())
}

/// Held on `HiveNode`. `host` is the trusted local-administration handle (create vault, add/
/// remove notes, grant readers); `reader` is this machine's own `LocalHub` session, used for
/// every read (list/search/read) so the desktop app exercises the exact same reader path a
/// second Private Fleet machine will use once cross-machine reading is wired up.
#[derive(Default)]
pub(crate) struct VaultState {
    host: Mutex<Option<LocalHubStore>>,
    reader: Mutex<Option<LocalHub>>,
    /// Set in `vault_open` when this machine has been `hive hub pair`-ed to another machine's
    /// hub (see `hive_core::local_hub::PairedHubCredentials`). When present, every *read*
    /// (vault_list/vault_search/vault_list_documents/vault_read) goes over this instead of
    /// `reader`'s own local session -- Jack, 2026-09-27: "no matter which machine I'm on I
    /// should be seeing the same data. This is our Private Fleet Library." `host` and `reader`
    /// are left exactly as before regardless of this field: this machine's own device
    /// enrollment and Bots owner binding are local-machine concepts unrelated to which vault its
    /// Library tab reads documents from, and still need a local vault-host.sqlite3 to live in.
    remote: Mutex<Option<RemoteLocalHub>>,
}
impl VaultState {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

#[uniffi::export]
impl HiveNode {
    /// Opens (creating on first use) this machine's vault store at
    /// `~/Library/Application Support/ohhive/vault-host.sqlite3` (Linux:
    /// `~/.config/ohhive/...`), enrolls this machine's own reader device the first time, and
    /// reconnects it. Safe to call repeatedly (e.g. on every launch, or when the Vault tab
    /// appears) -- a second call is a cheap no-op once already open.
    pub fn vault_open(&self) -> Result<VaultHostStatus, HiveError> {
        let mut host_guard = self.vault.host.lock().map_err(|_| poisoned())?;
        let first_open = host_guard.is_none();
        if first_open {
            *host_guard = Some(LocalHubStore::open(store_path()).map_err(HiveError::from)?);
        }
        let store = host_guard.as_ref().expect("just set").clone();
        drop(host_guard);
        // Opening the store marks every vault unavailable until its source revalidates (core
        // behavior, meant for folder-watched vaults). A hand-curated vault has no watcher to ever
        // undo that, so republish those specifically, once per real open -- not on every call.
        if first_open {
            store.vault_reopen_manual().map_err(HiveError::from)?;
            // Host owns this future (vault_maintenance.rs's own doc comment) -- nothing started
            // it before this. Fire-and-forget: runs for the process's lifetime, ticking any
            // vault whose policy is `enabled` (default off) on its own schedule. No stop signal
            // wired yet -- app exit is the only thing that ends it, which matches every other
            // per-launch host task in this file (the reader session, the owner enrollment).
            let maintenance_store = store.clone();
            RUNTIME.spawn(async move {
                let (_stop, rx) = tokio::sync::watch::channel(false);
                let _ = maintenance_store.vault_maintenance_run(rx).await;
            });
        }

        let mut reader_guard = self.vault.reader.lock().map_err(|_| poisoned())?;
        if reader_guard.is_none() {
            let raw_key = match nodeconfig::get_extra("HIVE_VAULT_SELF_KEY") {
                Some(k) => k,
                None => {
                    let creds = store
                        .enroll_owner("this machine")
                        .map_err(HiveError::from)?;
                    nodeconfig::set("HIVE_VAULT_SELF_KEY", &creds.raw_key)
                        .map_err(HiveError::from)?;
                    std::env::set_var("HIVE_VAULT_SELF_KEY", &creds.raw_key);
                    creds.raw_key
                }
            };
            *reader_guard = Some(store.connect(&raw_key).map_err(HiveError::from)?);
        }
        drop(reader_guard);

        // Paired-hub read path (see `VaultState::remote`'s doc). Built once and cached, same as
        // the local `reader` above -- `RemoteLocalHub::new` does no network I/O itself, so
        // building it doesn't tell us whether the hub is actually reachable; a call that follows
        // will.
        let mut remote_guard = self.vault.remote.lock().map_err(|_| poisoned())?;
        if remote_guard.is_none() {
            if let Some(creds) = hive_core::local_hub::read_paired_hub_credentials() {
                *remote_guard = Some(
                    RemoteLocalHub::new(&creds.hub_url, creds.raw_key).map_err(HiveError::from)?,
                );
            }
        }
        let remote = remote_guard.clone();
        drop(remote_guard);

        let vaults = match &remote {
            Some(r) => RUNTIME.block_on(r.vault_list()).map_err(HiveError::from)?,
            None => store.vault_list_all().map_err(HiveError::from)?,
        };
        Ok(VaultHostStatus {
            store_path: store_path().display().to_string(),
            vaults: vaults.into_iter().map(Into::into).collect(),
        })
    }

    /// Host-local administration only; this does not use the selected remote primary.
    pub fn vault_computer_access(
        &self,
        vault_id: String,
    ) -> Result<Vec<VaultComputerAccess>, HiveError> {
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let id = parse_uuid(&vault_id, "collection")?;
        Ok(host
            .vault_computer_access(id)
            .map_err(HiveError::from)?
            .into_iter()
            .map(|r| VaultComputerAccess {
                node_id: r.node_id.to_string(),
                name: r.name,
                allowed: r.allowed,
                active: r.active,
            })
            .collect())
    }
    pub fn vault_set_computer_access(
        &self,
        vault_id: String,
        node_id: String,
        allowed: bool,
    ) -> Result<(), HiveError> {
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let vault = parse_uuid(&vault_id, "collection")?;
        let node = parse_uuid(&node_id, "computer")?;
        host.vault_set_computer_access(vault, node, allowed)
            .map_err(HiveError::from)
    }

    /// Creates a vault and immediately grants this machine's own reader access to it -- see this
    /// file's header doc for why self-grant is automatic rather than a manual step. Marked ready
    /// right away: unlike a folder-backed vault (future work), a hand-curated one has no
    /// reconciliation window where its contents could be half-written.
    pub fn vault_create(&self, name: String) -> Result<VaultInfo, HiveError> {
        if self.remote_reader()?.is_some() {
            return Err(HiveError::Failed(
                "This machine reads the shared Library from another computer -- create new \
                 collections there instead."
                    .into(),
            ));
        }
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let reader = self
            .vault
            .reader
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let id = host.vault_create(&name).map_err(HiveError::from)?;
        let self_id = reader.node_id().map_err(HiveError::from)?;
        host.vault_grant(id, self_id, true)
            .map_err(HiveError::from)?;
        host.vault_set_available(id, true)
            .map_err(HiveError::from)?;
        Ok(VaultInfo {
            id: id.to_string(),
            name,
            state: "ready".to_string(),
        })
    }

    /// Sets (or updates) this vault's maintenance policy. Disabled by default -- `enabled: true`
    /// is what actually gets it picked up by the host loop `vault_open` now starts. Changing the
    /// policy cancels publication of any in-flight run for this vault (core behavior), it does
    /// not itself start anything -- the host loop is already running from `vault_open`.
    pub fn vault_configure_maintenance(
        &self,
        vault_id: String,
        policy: VaultMaintenancePolicy,
    ) -> Result<(), HiveError> {
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let v = parse_uuid(&vault_id, "vault id")?;
        host.vault_configure_maintenance(v, &policy.into())
            .map_err(HiveError::from)
    }

    /// Current policy, next-due time, whether a run is claimed right now, and the last result
    /// (if any) -- `None` means maintenance has never been configured for this vault at all.
    pub fn vault_maintenance_status(
        &self,
        vault_id: String,
    ) -> Result<Option<VaultMaintenanceStatus>, HiveError> {
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let v = parse_uuid(&vault_id, "vault id")?;
        Ok(host
            .vault_maintenance_status(v)
            .map_err(HiveError::from)?
            .map(Into::into))
    }

    /// Lists every vault this machine's own reader session can see, or -- once paired to another
    /// machine's hub -- every vault that hub's Library holds (see `remote_reader` below).
    pub fn vault_list(&self) -> Result<Vec<VaultInfo>, HiveError> {
        if let Some(r) = self.remote_reader()? {
            return Ok(RUNTIME
                .block_on(r.vault_list())
                .map_err(HiveError::from)?
                .into_iter()
                .map(Into::into)
                .collect());
        }
        let reader = self
            .vault
            .reader
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        Ok(reader
            .vault_list()
            .map_err(HiveError::from)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn vault_search(
        &self,
        vault_id: String,
        query: String,
        limit: u32,
    ) -> Result<Vec<VaultHit>, HiveError> {
        let vault = parse_uuid(&vault_id, "vault id")?;
        if let Some(r) = self.remote_reader()? {
            return Ok(RUNTIME
                .block_on(r.vault_search(vault, &query, limit))
                .map_err(HiveError::from)?
                .into_iter()
                .map(Into::into)
                .collect());
        }
        let reader = self
            .vault
            .reader
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        Ok(reader
            .vault_search(vault, &query, limit)
            .map_err(HiveError::from)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Lists a vault's documents without a search term -- backs the GUI's default browse view
    /// (VaultView.swift) instead of forcing a search first.
    pub fn vault_list_documents(
        &self,
        vault_id: String,
        limit: u32,
    ) -> Result<Vec<VaultHit>, HiveError> {
        let vault = parse_uuid(&vault_id, "vault id")?;
        if let Some(r) = self.remote_reader()? {
            return Ok(RUNTIME
                .block_on(r.vault_list_documents(vault, limit))
                .map_err(HiveError::from)?
                .into_iter()
                .map(Into::into)
                .collect());
        }
        let reader = self
            .vault
            .reader
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        Ok(reader
            .vault_list_documents(vault, limit)
            .map_err(HiveError::from)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn vault_read(
        &self,
        vault_id: String,
        document_id: String,
        revision: String,
    ) -> Result<VaultDocument, HiveError> {
        let vault = parse_uuid(&vault_id, "vault id")?;
        let id = parse_uuid(&document_id, "document id")?;
        if let Some(r) = self.remote_reader()? {
            return Ok(RUNTIME
                .block_on(r.vault_read(vault, id, &revision))
                .map_err(HiveError::from)?
                .into());
        }
        let reader = self
            .vault
            .reader
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        Ok(reader
            .vault_read(vault, id, &revision)
            .map_err(HiveError::from)?
            .into())
    }

    /// Adds (or, called again with the same `document_id`, edits) one Markdown note. `path` is a
    /// relative, slash-separated, `.md`-suffixed label the member makes up (e.g.
    /// `"recipes/lasagna.md"`) -- it is never a real filesystem path in this pass, just a stable
    /// display name; `document_id` is what actually identifies the note across edits. Pass
    /// `document_id: None` to create a new note (the returned `VaultDocument.id` is the one to
    /// pass back for later edits); there is no list-documents call in this pass, so the Swift UI
    /// should hold onto ids itself (e.g. in its note-editor state) rather than needing to look
    /// them up again -- search is the browse path for now.
    pub fn vault_add_note(
        &self,
        vault_id: String,
        document_id: Option<String>,
        path: String,
        title: String,
        content: String,
    ) -> Result<VaultDocument, HiveError> {
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let vault = parse_uuid(&vault_id, "vault id")?;
        let id = match document_id {
            Some(s) => parse_uuid(&s, "document id")?,
            None => Uuid::new_v4(),
        };
        let revision = host
            .vault_put(vault, id, &path, &title, &content)
            .map_err(HiveError::from)?;
        Ok(VaultDocument {
            id: id.to_string(),
            vault_id,
            path,
            revision,
            title,
            content,
        })
    }

    pub fn vault_remove_note(
        &self,
        vault_id: String,
        document_id: String,
    ) -> Result<(), HiveError> {
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let vault = parse_uuid(&vault_id, "vault id")?;
        let id = parse_uuid(&document_id, "document id")?;
        host.vault_remove_document(vault, id)
            .map_err(HiveError::from)
    }

    /// Renames a collection. GUI: right-click a collection title -> "Rename…".
    pub fn vault_rename(&self, vault_id: String, name: String) -> Result<(), HiveError> {
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let vault = parse_uuid(&vault_id, "vault id")?;
        host.vault_rename(vault, &name).map_err(HiveError::from)
    }

    /// Permanently deletes a collection and everything in it. The confirmation dialog is a
    /// UI-layer safeguard (GUI: right-click a collection title -> "Delete", then confirm) -- this
    /// call itself is unconditional and cannot be undone once made.
    pub fn vault_delete(&self, vault_id: String) -> Result<(), HiveError> {
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let vault = parse_uuid(&vault_id, "vault id")?;
        host.vault_delete(vault).map_err(HiveError::from)
    }

    /// Lists `.md` files under `root` (an absolute path on this machine) for a member to review
    /// before approving any of them for library intake. Read-only -- never touches the vault
    /// store, never submits anything
    /// (`hive_core::local_hub::vault_intake_folder::vault_intake_list_candidates`).
    pub fn vault_intake_list_candidates(
        &self,
        root: String,
    ) -> Result<Vec<IntakeCandidate>, HiveError> {
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        Ok(host
            .vault_intake_list_candidates(&root)
            .map_err(HiveError::from)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Submits one member-approved file from `root` into `vault_id` (must already be a managed,
    /// non-folder vault -- the core call rejects a folder-attached one). `relative_path` should
    /// be one `vault_intake_list_candidates` returned for the same `root`; `project` is an
    /// optional slug used purely to group the generated document under `Intake/<project>/...`.
    pub fn vault_intake_approve_file(
        &self,
        vault_id: String,
        root: String,
        relative_path: String,
        project: Option<String>,
    ) -> Result<IntakeReceipt, HiveError> {
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let vault = parse_uuid(&vault_id, "vault id")?;
        Ok(host
            .vault_intake_approve_file(vault, &root, &relative_path, project.as_deref())
            .map_err(HiveError::from)?
            .into())
    }

    /// Same as `vault_intake_list_candidates` but pre-filtered to well-known agent/assistant
    /// instruction filenames (soul.md, CLAUDE.md, AGENTS.md, agent.md) -- backs onboarding's
    /// "scan for agent instructions" prompt.
    pub fn vault_agent_instruction_candidates(
        &self,
        root: String,
    ) -> Result<Vec<IntakeCandidate>, HiveError> {
        let host = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        Ok(host
            .vault_agent_instruction_candidates(&root)
            .map_err(HiveError::from)?
            .into_iter()
            .map(Into::into)
            .collect())
    }
}

impl HiveNode {
    /// Every vault read method below checks this first -- set only when this machine is paired
    /// to another machine's hub (see `VaultState::remote`'s doc) -- and falls back to the local
    /// `reader` session when it's `None`, so a never-paired or hub-hosting machine (e.g. Asgard
    /// itself) behaves exactly as before. Not `#[uniffi::export]`-ed: `RemoteLocalHub` has no
    /// UniFFI bindings of its own (nothing outside this file needs to see it), and putting a
    /// private helper in the exported `impl HiveNode` block above makes the export macro try to
    /// bridge it anyway.
    fn remote_reader(&self) -> Result<Option<RemoteLocalHub>, HiveError> {
        Ok(self.vault.remote.lock().map_err(|_| poisoned())?.clone())
    }

    /// Called only after bots_open verifies this machine's member with HubClient::whoami.
    /// The local reader node UUID differs from the cloud node UUID: bind the actual local key.
    pub(crate) fn bind_bots_owner(&self, member: Uuid) -> Result<LocalHubStore, HiveError> {
        self.vault_open()?;
        let store_guard = self.vault.host.lock().map_err(|_| poisoned())?;
        let reader_guard = self.vault.reader.lock().map_err(|_| poisoned())?;
        let store = store_guard.as_ref().ok_or_else(not_open)?;
        let reader = reader_guard.as_ref().ok_or_else(not_open)?;
        store
            .set_node_owner(reader.node_id().map_err(HiveError::from)?, member)
            .map_err(HiveError::from)?;
        Ok(store.clone())
    }
}

#[uniffi::export]
impl HiveNode {
    /// Produces a public, five-minute connection request; never returns the device key.
    pub fn private_fleet_enrollment_begin(&self) -> Result<String, HiveError> {
        self.vault_open()?;
        let reader = self
            .vault
            .reader
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        serde_json::to_string(&reader.enrollment_challenge().map_err(HiveError::from)?)
            .map_err(|_| HiveError::Failed("Cannot create connection request".into()))
    }

    /// Trust comes only from local platform configuration, never from approval contents.
    pub fn private_fleet_enrollment_complete(&self, approval: String) -> Result<(), HiveError> {
        use hive_core::local_hub::enrollment::{EnrollmentAssertion, EnrollmentTrust};
        if approval.len() > 10000 {
            return Err(HiveError::Failed("Invalid enrollment approval".into()));
        }
        let assertion: EnrollmentAssertion = serde_json::from_str(&approval)
            .map_err(|_| HiveError::Failed("Invalid enrollment approval".into()))?;
        self.vault_open()?;
        let store = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let reader = self
            .vault
            .reader
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        if reader
            .private_fleet_identity()
            .map_err(HiveError::from)?
            .is_some()
        {
            reader
                .enrollment_complete(assertion)
                .map_err(HiveError::from)?;
        } else {
            let setting = |name: &str| {
                nodeconfig::get_extra(name).ok_or_else(|| {
                    HiveError::Failed(
                        "Private Fleet sign-in is not configured on this installation yet".into(),
                    )
                })
            };
            let (issuer, key_id, public_key) = nodeconfig::private_fleet_trust_defaults();
            let trust = EnrollmentTrust {
                issuer,
                key_id,
                public_key,
            };
            let key = setting("HIVE_VAULT_SELF_KEY")?;
            store
                .configure_private_fleet(trust, assertion, &key)
                .map_err(HiveError::from)?;
        }
        Ok(())
    }
}
impl HiveNode {
    pub(crate) fn private_bots_context(
        &self,
    ) -> Result<Option<(LocalHubStore, Uuid, Uuid, String)>, HiveError> {
        self.vault_open()?;
        let store = self
            .vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let reader = self
            .vault
            .reader
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)?;
        let Some(identity) = reader.private_fleet_identity().map_err(HiveError::from)? else {
            return Ok(None);
        };
        let key = nodeconfig::get_extra("HIVE_VAULT_SELF_KEY").ok_or_else(not_open)?;
        Ok(Some((store, identity.owner_id, identity.node_id, key)))
    }
}

impl HiveNode {
    pub(crate) fn private_fleet_is_enrolled(&self) -> Result<bool, HiveError> {
        if crate::private_fleet::selected()?.is_some() {
            return Ok(true);
        }
        if !store_path().exists() {
            return Ok(false);
        }
        Ok(self.private_bots_context()?.is_some())
    }
}

impl HiveNode {
    pub(crate) fn private_primary_store(&self) -> Result<LocalHubStore, HiveError> {
        self.vault_open()?;
        self.vault
            .host
            .lock()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(not_open)
    }
}
