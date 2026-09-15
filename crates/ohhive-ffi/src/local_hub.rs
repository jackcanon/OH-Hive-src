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
//! **Deliberately NOT in this pass:** reading a vault from a *second* Private Fleet machine.
//! That needs `local_hub::transport::serve()` (host side, a LAN/tunnel-bound HTTP listener) and
//! `RemoteLocalHub::pair()`/`vault_grant()` with a real cross-device consent step (the vault
//! grants nothing on pairing alone, by design -- see `vault.rs`'s doc comment). The storage and
//! HTTP-reader plumbing for that already exists and is tested (Sif's central-query v1 report);
//! this file's plain in-process `LocalHub` reader exists specifically so that plumbing is a
//! small, additive follow-up (swap in a `RemoteLocalHub` branch) rather than a rewrite.
//!
//! All state here is synchronous (SQLite on the local disk, no network), so unlike most of this
//! crate's methods these are plain `pub fn`, not `async fn` -- no reason to touch `RUNTIME`.

use crate::{HiveError, HiveNode};
use hive_core::local_hub::vault::{VaultDocument as CoreDoc, VaultHit as CoreHit, VaultInfo as CoreInfo};
use hive_core::local_hub::{LocalHub, LocalHubStore};
use hive_core::local_hub::vault_intake::IntakeReceipt as CoreReceipt;
use hive_core::local_hub::vault_intake_folder::IntakeCandidate as CoreCandidate;
use hive_core::nodeconfig;
use std::sync::Mutex;
use uuid::Uuid;

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
        Self { id: v.id.to_string(), name: v.name, state: v.state }
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
        Self { id: h.id.to_string(), path: h.path, revision: h.revision, title: h.title, snippet: h.snippet, score: h.score }
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
        Self { relative_path: c.relative_path, title: c.title, size: c.size }
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
        Self { document_id: r.document_id.to_string(), revision: r.revision, unchanged: r.unchanged }
    }
}

fn store_path() -> std::path::PathBuf {
    nodeconfig::path().with_file_name("vault-host.sqlite3")
}
fn parse_uuid(s: &str, what: &str) -> Result<Uuid, HiveError> {
    Uuid::parse_str(s).map_err(|_| HiveError::Failed(format!("not a valid {what}")))
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
        }

        let mut reader_guard = self.vault.reader.lock().map_err(|_| poisoned())?;
        if reader_guard.is_none() {
            let raw_key = match nodeconfig::get_extra("HIVE_VAULT_SELF_KEY") {
                Some(k) => k,
                None => {
                    let creds = store.enroll_owner("this machine").map_err(HiveError::from)?;
                    nodeconfig::set("HIVE_VAULT_SELF_KEY", &creds.raw_key).map_err(HiveError::from)?;
                    std::env::set_var("HIVE_VAULT_SELF_KEY", &creds.raw_key);
                    creds.raw_key
                }
            };
            *reader_guard = Some(store.connect(&raw_key).map_err(HiveError::from)?);
        }
        drop(reader_guard);

        let vaults = store.vault_list_all().map_err(HiveError::from)?;
        Ok(VaultHostStatus {
            store_path: store_path().display().to_string(),
            vaults: vaults.into_iter().map(Into::into).collect(),
        })
    }

    /// Creates a vault and immediately grants this machine's own reader access to it -- see this
    /// file's header doc for why self-grant is automatic rather than a manual step. Marked ready
    /// right away: unlike a folder-backed vault (future work), a hand-curated one has no
    /// reconciliation window where its contents could be half-written.
    pub fn vault_create(&self, name: String) -> Result<VaultInfo, HiveError> {
        let host = self.vault.host.lock().map_err(|_| poisoned())?.clone().ok_or_else(not_open)?;
        let reader = self.vault.reader.lock().map_err(|_| poisoned())?.clone().ok_or_else(not_open)?;
        let id = host.vault_create(&name).map_err(HiveError::from)?;
        let self_id = reader.node_id().map_err(HiveError::from)?;
        host.vault_grant(id, self_id, true).map_err(HiveError::from)?;
        host.vault_set_available(id, true).map_err(HiveError::from)?;
        Ok(VaultInfo { id: id.to_string(), name, state: "ready".to_string() })
    }

    /// Lists every vault this machine's own reader session can see -- today that is every vault
    /// this machine has created (self-grant is automatic), since cross-machine grants aren't
    /// wired up yet (see header doc).
    pub fn vault_list(&self) -> Result<Vec<VaultInfo>, HiveError> {
        let reader = self.vault.reader.lock().map_err(|_| poisoned())?.clone().ok_or_else(not_open)?;
        Ok(reader.vault_list().map_err(HiveError::from)?.into_iter().map(Into::into).collect())
    }

    pub fn vault_search(&self, vault_id: String, query: String, limit: u32) -> Result<Vec<VaultHit>, HiveError> {
        let reader = self.vault.reader.lock().map_err(|_| poisoned())?.clone().ok_or_else(not_open)?;
        let vault = parse_uuid(&vault_id, "vault id")?;
        Ok(reader
            .vault_search(vault, &query, limit)
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
        let reader = self.vault.reader.lock().map_err(|_| poisoned())?.clone().ok_or_else(not_open)?;
        let vault = parse_uuid(&vault_id, "vault id")?;
        let id = parse_uuid(&document_id, "document id")?;
        Ok(reader.vault_read(vault, id, &revision).map_err(HiveError::from)?.into())
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
        let host = self.vault.host.lock().map_err(|_| poisoned())?.clone().ok_or_else(not_open)?;
        let vault = parse_uuid(&vault_id, "vault id")?;
        let id = match document_id {
            Some(s) => parse_uuid(&s, "document id")?,
            None => Uuid::new_v4(),
        };
        let revision = host.vault_put(vault, id, &path, &title, &content).map_err(HiveError::from)?;
        Ok(VaultDocument { id: id.to_string(), vault_id, path, revision, title, content })
    }

    pub fn vault_remove_note(&self, vault_id: String, document_id: String) -> Result<(), HiveError> {
        let host = self.vault.host.lock().map_err(|_| poisoned())?.clone().ok_or_else(not_open)?;
        let vault = parse_uuid(&vault_id, "vault id")?;
        let id = parse_uuid(&document_id, "document id")?;
        host.vault_remove_document(vault, id).map_err(HiveError::from)
    }

    /// Lists `.md` files under `root` (an absolute path on this machine) for a member to review
    /// before approving any of them for library intake. Read-only -- never touches the vault
    /// store, never submits anything
    /// (`hive_core::local_hub::vault_intake_folder::vault_intake_list_candidates`).
    pub fn vault_intake_list_candidates(&self, root: String) -> Result<Vec<IntakeCandidate>, HiveError> {
        let host = self.vault.host.lock().map_err(|_| poisoned())?.clone().ok_or_else(not_open)?;
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
        let host = self.vault.host.lock().map_err(|_| poisoned())?.clone().ok_or_else(not_open)?;
        let vault = parse_uuid(&vault_id, "vault id")?;
        Ok(host
            .vault_intake_approve_file(vault, &root, &relative_path, project.as_deref())
            .map_err(HiveError::from)?
            .into())
    }
}
