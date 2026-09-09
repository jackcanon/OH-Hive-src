//! Hub client — how a node talks to the source of record (ADR-001) without
//! ever holding a Supabase user JWT (ADR-004). Auth is a *node key* minted by
//! the owning member (migration 0002); every call is a PostgREST RPC.
//!
//! Endpoints are the `public.hive_*` wrappers until schema `hive` is exposed
//! in the project's API settings, after which `Content-Profile: hive` and the
//! unprefixed names work too.

use crate::capability::{Capabilities, ToolsLevel};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum HubError {
    #[error("hub unreachable: {0}")]
    Transport(String),
    #[error("hub rejected: {0}")]
    Rejected(String),
    #[error("invalid or revoked node key")]
    BadKey,
}

#[derive(Clone)]
pub struct HubClient {
    base: String,
    anon_key: String,
    node_key: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhoAmI {
    pub node_id: Uuid,
    pub display_name: String,
    pub role: String,
    pub region: String,
    pub presence: String,
    pub member_id: Uuid,
}

impl HubClient {
    /// `base` is the Supabase project URL, e.g. `https://xyz.supabase.co`.
    pub fn new(
        base: impl Into<String>,
        anon_key: impl Into<String>,
        node_key: impl Into<String>,
    ) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            anon_key: anon_key.into(),
            node_key: node_key.into(),
            http: reqwest::Client::new(),
        }
    }

    async fn rpc<T: for<'de> Deserialize<'de>>(
        &self,
        name: &str,
        body: serde_json::Value,
    ) -> Result<T, HubError> {
        let resp = self
            .http
            .post(format!("{}/rest/v1/rpc/{}", self.base, name))
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.anon_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| HubError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| HubError::Transport(e.to_string()))?;
        if !status.is_success() {
            if text.contains("invalid_or_revoked_node_key") {
                return Err(HubError::BadKey);
            }
            return Err(HubError::Rejected(format!("{status}: {text}")));
        }
        serde_json::from_str(&text)
            .map_err(|e| HubError::Rejected(format!("bad response: {e}: {text}")))
    }

    pub async fn whoami(&self) -> Result<WhoAmI, HubError> {
        self.rpc(
            "hive_node_whoami",
            serde_json::json!({ "raw_key": self.node_key }),
        )
        .await
    }

    /// Publish capabilities and become eligible for work. Returns the node row as JSON.
    pub async fn check_in(
        &self,
        caps: &Capabilities,
        region: Option<&str>,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_node_checkin",
            serde_json::json!({ "raw_key": self.node_key, "p_capabilities": caps, "p_region": region }),
        )
        .await
    }

    pub async fn heartbeat(&self) -> Result<String, HubError> {
        self.rpc(
            "hive_node_heartbeat",
            serde_json::json!({ "raw_key": self.node_key }),
        )
        .await
    }

    /// Returns the resulting presence: "checked_out" or "draining" (lease held).
    pub async fn check_out(&self) -> Result<String, HubError> {
        self.rpc(
            "hive_node_checkout",
            serde_json::json!({ "raw_key": self.node_key }),
        )
        .await
    }

    // ── Dispatch (ADR-005 leases; v0 pull model) ─────────────────────────────

    /// Ask the hub for one card this node is eligible for. `Leased` carries the
    /// card, its project, and the latest output of each dependency.
    pub async fn claim_card(&self) -> Result<Claim, HubError> {
        self.rpc(
            "hive_node_claim_card",
            serde_json::json!({ "raw_key": self.node_key }),
        )
        .await
    }

    /// Report a finished card. The hub stores the output, meters `usage` into
    /// $honey at the current rate (project fund → this node's owner wallet), and
    /// moves the card to `review`.
    pub async fn complete_card(
        &self,
        card_id: Uuid,
        content: &str,
        model_id: Option<&str>,
        usage: crate::ledger::Usage,
    ) -> Result<Completion, HubError> {
        self.rpc(
            "hive_node_complete_card",
            serde_json::json!({
                "raw_key": self.node_key, "p_card_id": card_id, "p_content": content, "p_model_id": model_id,
                "p_tokens_in": usage.tokens_in, "p_tokens_out": usage.tokens_out, "p_compute_seconds": usage.compute_seconds,
            }),
        )
        .await
    }

    /// Persist agent-loop state at a step boundary and extend the lease (ADR-006 D42).
    pub async fn checkpoint(
        &self,
        card_id: Uuid,
        step: u32,
        state: &serde_json::Value,
        usage: crate::ledger::Usage,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_node_checkpoint",
            serde_json::json!({
                "raw_key": self.node_key, "p_card_id": card_id, "p_step": step, "p_state": state,
                "p_usage": { "tokens_in": usage.tokens_in, "tokens_out": usage.tokens_out, "compute_seconds": usage.compute_seconds },
            }),
        )
        .await
    }

    pub async fn fail_card(
        &self,
        card_id: Uuid,
        reason: &str,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_node_fail_card",
            serde_json::json!({ "raw_key": self.node_key, "p_card_id": card_id, "p_reason": reason }),
        )
        .await
    }

    // ── regional server (ADR-004/007, v0) ──────────────────────────────────────────────────

    pub async fn server_register(
        &self,
        req: &ServerRegistration,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_server_register",
            serde_json::json!({
                "raw_key": self.node_key, "p_public_url": req.public_url, "p_multiaddrs": req.multiaddrs,
                "p_operator": req.operator, "p_tier": req.tier, "p_storage_gb": req.storage_gb,
                "p_region": req.region, "p_version": crate::VERSION,
            }),
        )
        .await
    }

    pub async fn server_heartbeat(
        &self,
        storage_used_bytes: u64,
        connections: u32,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_server_heartbeat",
            serde_json::json!({ "raw_key": self.node_key, "p_storage_used_bytes": storage_used_bytes, "p_connections": connections }),
        )
        .await
    }

    /// Announce that this server now holds blob `hash`.
    pub async fn artifact_announce(
        &self,
        a: &ArtifactAnnounce,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_artifact_announce",
            serde_json::json!({
                "raw_key": self.node_key, "p_hash": a.hash, "p_bytes": a.bytes, "p_mime": a.mime, "p_kind": a.kind,
                "p_project_id": a.project_id, "p_card_id": a.card_id, "p_uploaded_by": a.uploaded_by,
            }),
        )
        .await
    }

    /// Where can I upload/fetch artifacts? Node-key-gated twin of the member-JWT-only
    /// `hive.artifact_locate`/`hive.servers` (ADR-006's `artifact_get`/`artifact_put`).
    /// `hash = None` → online regional servers to upload to; `hash = Some(h)` → who already
    /// holds `h`, with ready-to-fetch `GET /a/<hash>` URLs, nearest region first.
    pub async fn artifact_locate(&self, hash: Option<&str>) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_node_artifact_locate",
            serde_json::json!({ "raw_key": self.node_key, "p_hash": hash }),
        )
        .await
    }

    /// Fetch an artifact's bytes: locate it, then `GET` from the nearest replica that answers.
    /// Tries every URL `artifact_locate` returns (already ordered nearest-region-first) before
    /// giving up — a card shouldn't fail just because one of two replicas is briefly offline.
    pub async fn artifact_fetch(&self, hash: &str) -> Result<(Vec<u8>, String), HubError> {
        let located = self.artifact_locate(Some(hash)).await?;
        let urls = located
            .get("urls")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if urls.is_empty() {
            return Err(HubError::Rejected(format!(
                "no replica holds artifact {hash}"
            )));
        }
        let mime = located
            .get("artifact")
            .and_then(|a| a.get("mime"))
            .and_then(|v| v.as_str())
            .unwrap_or("application/octet-stream")
            .to_string();
        let mut last_err = String::new();
        for u in urls.iter().filter_map(|v| v.as_str()) {
            match self.http.get(u).send().await {
                Ok(resp) if resp.status().is_success() => {
                    return match resp.bytes().await {
                        Ok(b) => Ok((b.to_vec(), mime)),
                        Err(e) => {
                            last_err = e.to_string();
                            continue;
                        }
                    };
                }
                Ok(resp) => last_err = format!("{u}: {}", resp.status()),
                Err(e) => last_err = format!("{u}: {e}"),
            }
        }
        Err(HubError::Transport(format!(
            "every replica of {hash} failed; last error: {last_err}"
        )))
    }

    /// Upload bytes to whichever online regional server is nearest, using the same content-
    /// addressed `PUT /a` protocol `hive-server`'s own `put_blob` handler expects. The server
    /// hashes, stores, and self-announces to the hub (`hive.artifact_announce`) — this call
    /// never touches that RPC directly. Returns the server's response (`hash`, `bytes`, `mime`).
    pub async fn artifact_upload(
        &self,
        bytes: Vec<u8>,
        mime: &str,
        kind: &str,
        project_id: Option<Uuid>,
        card_id: Option<Uuid>,
    ) -> Result<serde_json::Value, HubError> {
        let located = self.artifact_locate(None).await?;
        let servers = located
            .get("servers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if servers.is_empty() {
            return Err(HubError::Rejected(
                "no online regional server has a public URL to upload to".into(),
            ));
        }
        let mut last_err = String::new();
        for s in &servers {
            let Some(url) = s.get("public_url").and_then(|v| v.as_str()) else {
                continue;
            };
            let mut req = self
                .http
                .put(format!("{}/a", url.trim_end_matches('/')))
                .header("Authorization", format!("Bearer {}", self.node_key))
                .header("Content-Type", mime)
                .header("X-Hive-Kind", kind)
                .body(bytes.clone());
            if let Some(p) = project_id {
                req = req.header("X-Hive-Project", p.to_string());
            }
            if let Some(c) = card_id {
                req = req.header("X-Hive-Card", c.to_string());
            }
            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    let text = resp.text().await.unwrap_or_default();
                    return serde_json::from_str(&text)
                        .map_err(|e| HubError::Rejected(format!("bad upload reply: {e}: {text}")));
                }
                Ok(resp) => last_err = format!("{url}: {}", resp.status()),
                Err(e) => last_err = format!("{url}: {e}"),
            }
        }
        Err(HubError::Transport(format!(
            "every regional server rejected the upload; last error: {last_err}"
        )))
    }

    /// Create a child card in the same project as `parent_card_id` (ADR-006 D44). Only the node
    /// currently holding the parent's lease may call this. `requires_internet` is not a
    /// parameter — the hub always inherits it from the parent (D47: a child can't widen it).
    /// This creates the card only; nothing yet makes the parent wait for it to finish (see
    /// `crate::tools`' module doc).
    #[allow(clippy::too_many_arguments)] // one flat RPC payload; a params struct would just move the same 7 fields, not reduce them
    pub async fn spawn_child_card(
        &self,
        parent_card_id: Uuid,
        key: &str,
        title: &str,
        modality: &str,
        inputs: &str,
        acceptance: &str,
        required_capabilities: serde_json::Value,
    ) -> Result<SpawnedCard, HubError> {
        self.rpc(
            "hive_spawn_child_card",
            serde_json::json!({
                "raw_key": self.node_key,
                "p_parent_card_id": parent_card_id,
                "p_key": key,
                "p_title": title,
                "p_modality": modality,
                "p_inputs": inputs,
                "p_acceptance": acceptance,
                "p_required_capabilities": required_capabilities,
            }),
        )
        .await
    }

    /// Release this card's lease and mark it `waiting_on_child` instead of `ready` (ADR-006
    /// D44 sub-delegation) — `hive.node_claim_card` only ever claims `ready` cards, so this
    /// takes the card off the board until a DB trigger flips it back once `child_card_id`
    /// (and every other card this one spawned) reaches `review`/`done`, or cascades a
    /// `blocked` up immediately if the child fails instead. The child's output, once ready,
    /// arrives through the *same* `dep_outputs` a resumed claim already carries — no separate
    /// fetch call needed.
    pub async fn wait_on_child(
        &self,
        card_id: Uuid,
        child_card_id: Uuid,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_node_wait_on_child",
            serde_json::json!({
                "raw_key": self.node_key,
                "p_card_id": card_id,
                "p_child_card_id": child_card_id,
            }),
        )
        .await
    }

    /// Compete for / renew the coordinator lease (ADR-005 §1). Returns `coordinator: true` if we hold it.
    pub async fn coordinator_try(&self, ttl_seconds: u32) -> Result<CoordinatorLease, HubError> {
        let v: serde_json::Value = self
            .rpc(
                "hive_coordinator_try",
                serde_json::json!({ "raw_key": self.node_key, "p_ttl_seconds": ttl_seconds }),
            )
            .await?;
        serde_json::from_value(v).map_err(|e| HubError::Rejected(format!("bad lease reply: {e}")))
    }

    /// What this server should fetch to bring artifacts up to their replication factor (ADR-007).
    pub async fn replication_plan(&self, limit: u32) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_replication_plan",
            serde_json::json!({ "raw_key": self.node_key, "p_limit": limit }),
        )
        .await
    }

    /// HJM-operated servers only: the full hive schema as JSON for a nightly backup (ADR-013 D73).
    pub async fn backup_export(&self) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_backup_export",
            serde_json::json!({ "raw_key": self.node_key }),
        )
        .await
    }

    /// Announce a stored, encrypted backup blob as kind='backup' (pinned, replication 3).
    pub async fn backup_record(
        &self,
        hash: &str,
        bytes: u64,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_backup_record",
            serde_json::json!({ "raw_key": self.node_key, "p_hash": hash, "p_bytes": bytes }),
        )
        .await
    }

    /// The oldest whole calendar month (as an ISO timestamp) still waiting to be archived, if any
    /// (ADR-013 D73). `None` means nothing is more than 90 days old yet.
    pub async fn ledger_archive_pending(&self) -> Result<Option<String>, HubError> {
        let v: serde_json::Value = self
            .rpc("hive_ledger_archive_pending", serde_json::json!({}))
            .await?;
        Ok(v.as_str().map(str::to_string))
    }

    /// HJM-operated servers only: every hive.ledger_entries row in `[month_start, month_start+1mo)`
    /// as JSON, for the node to turn into a signed Parquet artifact (ADR-013 D73).
    pub async fn ledger_archive_export(
        &self,
        month_start: &str,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_ledger_archive_export",
            serde_json::json!({ "raw_key": self.node_key, "p_month_start": month_start }),
        )
        .await
    }

    /// Pin the archive artifact, checkpoint every touched account as of month end, and delete the
    /// now-archived hot rows in one transaction. `entry_count` must match what was exported, or the
    /// hub aborts the whole thing rather than risk losing entries.
    pub async fn ledger_archive_apply(
        &self,
        month_start: &str,
        hash: &str,
        bytes: u64,
        entry_count: u64,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_ledger_archive_apply",
            serde_json::json!({
                "raw_key": self.node_key, "p_month_start": month_start, "p_hash": hash,
                "p_bytes": bytes, "p_entry_count": entry_count,
            }),
        )
        .await
    }

    /// Which of the blobs this server holds may be deleted (no artifact row, returned, or unpinned past grace).
    pub async fn gc_plan(&self, hashes: &[String]) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_gc_plan",
            serde_json::json!({ "raw_key": self.node_key, "p_hashes": hashes }),
        )
        .await
    }

    /// This server no longer holds `hash`.
    pub async fn replica_drop(&self, hash: &str) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_replica_drop",
            serde_json::json!({ "raw_key": self.node_key, "p_hash": hash }),
        )
        .await
    }

    /// This node's record, what it has earned, and its owner's wallet — the desktop app's front page.
    pub async fn node_summary(&self) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_node_summary",
            serde_json::json!({ "raw_key": self.node_key }),
        )
        .await
    }

    /// Every Hive project, node-scoped read (owner/role omitted, same shape the web app's public
    /// board shows minus per-member fields) -- lets the native Swift app's Kanban view (Good Idea
    /// Fairy: "local hive or OH Hive") list real cloud projects without needing a member JWT the
    /// app never holds (ADR-004). See `hive.node_projects_overview` migration for the security
    /// reasoning -- same shape as `node_summary`, "nothing about other members" preserved by
    /// omitting `my_role` entirely rather than guessing at it.
    pub async fn node_projects_overview(&self) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_node_projects_overview",
            serde_json::json!({ "raw_key": self.node_key }),
        )
        .await
    }

    /// Coordinator-only: the read-all snapshot document (ADR-013 §A.5).
    pub async fn snapshot_source(&self) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_snapshot_source",
            serde_json::json!({ "raw_key": self.node_key }),
        )
        .await
    }

    /// Step down as coordinator (graceful shutdown).
    pub async fn coordinator_release(&self) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_coordinator_release",
            serde_json::json!({ "raw_key": self.node_key }),
        )
        .await
    }

    /// Verify some *other* node's key (an uploader) by asking the hub who it is.
    pub async fn whoami_for(&self, other_key: &str) -> Result<WhoAmI, HubError> {
        let v: serde_json::Value = self
            .rpc(
                "hive_node_whoami",
                serde_json::json!({ "raw_key": other_key }),
            )
            .await?;
        serde_json::from_value(v).map_err(|e| HubError::Rejected(format!("bad whoami: {e}")))
    }

    /// Hand a leased card back to the queue (card → ready, lease dropped, checkpoints kept so the
    /// next claimant resumes). Used on graceful shutdown mid-card.
    pub async fn release_card(
        &self,
        card_id: Uuid,
        reason: &str,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_node_release_card",
            serde_json::json!({ "raw_key": self.node_key, "p_card_id": card_id, "p_reason": reason }),
        )
        .await
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ServerRegistration {
    pub public_url: String,
    pub multiaddrs: Vec<String>,
    pub operator: String, // volunteer | hjm
    pub tier: String,     // primary | standby
    pub storage_gb: Option<u32>,
    pub region: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CoordinatorLease {
    pub coordinator: bool,
    pub holder: Option<Uuid>,
    pub holder_name: Option<String>,
    #[serde(default)]
    pub holder_url: Option<String>,
    pub expires_at: Option<String>,
    pub generation: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpawnedCard {
    pub card_id: Uuid,
    pub key: String,
    pub project_id: Uuid,
    pub requires_internet: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ArtifactAnnounce {
    pub hash: String,
    pub bytes: u64,
    pub mime: String,
    pub kind: String,
    pub project_id: Option<Uuid>,
    pub card_id: Option<Uuid>,
    pub uploaded_by: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaimedCard {
    pub id: Uuid,
    pub project_id: Uuid,
    pub key: String,
    pub title: String,
    pub modality: String,
    pub inputs: String,
    pub acceptance: String,
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub requires_internet: bool,
    #[serde(default)]
    pub required_capabilities: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaimedProject {
    pub id: Uuid,
    pub title: String,
    pub goal: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // one transient value per poll; boxing buys nothing
pub enum Claim {
    NothingToDo,
    NotCheckedIn,
    AlreadyLeased,
    Leased {
        card: ClaimedCard,
        project: ClaimedProject,
        #[serde(default)]
        dep_outputs: serde_json::Map<String, serde_json::Value>,
        /// Latest checkpoint from a previous (dead) holder, if any — resume from it.
        #[serde(default)]
        checkpoint: Option<CheckpointRecord>,
        lease_expires_at: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckpointRecord {
    pub step: u32,
    pub blob_hash: String,
    pub usage: crate::ledger::Usage,
    pub state: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Completion {
    pub status: String,
    pub earned_honey: f64,
    pub fund_balance: f64,
    pub wallet_balance: f64,
}

// ── Pairing (device-authorization onboarding) ────────────────────────────────
// A node without a key calls `pair_begin`, shows the code, and polls `pair_poll`
// until the member claims the code on the web app. See migration 0003.

#[derive(Debug, Clone, Deserialize)]
pub struct PairingStart {
    pub code: String,
    pub secret: String,
    pub expires_in_seconds: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PairingPoll {
    Pending,
    Expired,
    Claimed {
        node_key: String,
        node_id: Uuid,
        display_name: String,
        /// What the member chose on the web pairing page (ADR-006 D46/D48) — the caller
        /// should seed local node.env with these before ever calling check-in, since
        /// check-in always sends the local values and would otherwise reset them to
        /// their defaults.
        allow_internet: bool,
        tools_level: ToolsLevel,
    },
}

/// Calls member RPCs *as the member* (their Supabase JWT), so RLS applies. Used by the regional
/// server's live broadcast to read a board on a subscriber's behalf (ADR-013 §A.4).
pub struct MemberClient {
    base: String,
    anon_key: String,
    http: reqwest::Client,
}

impl MemberClient {
    pub fn new(base: impl Into<String>, anon_key: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            anon_key: anon_key.into(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn rpc(
        &self,
        jwt: &str,
        name: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, HubError> {
        let resp = self
            .http
            .post(format!("{}/rest/v1/rpc/{}", self.base, name))
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {jwt}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| HubError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| HubError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(HubError::Rejected(format!("{status}: {text}")));
        }
        serde_json::from_str(&text)
            .map_err(|e| HubError::Rejected(format!("bad response: {e}: {text}")))
    }
}

/// Unauthenticated pairing client (no node key yet).
pub struct Pairing {
    base: String,
    anon_key: String,
    http: reqwest::Client,
}

impl Pairing {
    pub fn new(base: impl Into<String>, anon_key: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            anon_key: anon_key.into(),
            http: reqwest::Client::new(),
        }
    }

    async fn rpc<T: for<'de> Deserialize<'de>>(
        &self,
        name: &str,
        body: serde_json::Value,
    ) -> Result<T, HubError> {
        let resp = self
            .http
            .post(format!("{}/rest/v1/rpc/{}", self.base, name))
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.anon_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| HubError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| HubError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(HubError::Rejected(format!("{status}: {text}")));
        }
        serde_json::from_str(&text)
            .map_err(|e| HubError::Rejected(format!("bad response: {e}: {text}")))
    }

    pub async fn begin(&self, hint: serde_json::Value) -> Result<PairingStart, HubError> {
        self.rpc("hive_pair_begin", serde_json::json!({ "p_hint": hint }))
            .await
    }

    pub async fn poll(&self, secret: &str) -> Result<PairingPoll, HubError> {
        self.rpc("hive_pair_poll", serde_json::json!({ "p_secret": secret }))
            .await
    }
}
