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
pub struct SupabaseHub {
    base: String,
    anon_key: String,
    node_key: String,
    http: reqwest::Client,
}

/// Backward-compatible name for account/community-only call sites.
pub type HubClient = SupabaseHub;

/// The private-project data plane. Implementations must not silently fall back to a community hub.
#[async_trait::async_trait]
pub trait Hub: Send + Sync {
    async fn claim_card(&self) -> Result<Claim, HubError>;
    async fn complete_card(
        &self,
        card_id: Uuid,
        content: &str,
        model_id: Option<&str>,
        usage: crate::ledger::Usage,
    ) -> Result<Completion, HubError>;
    async fn checkpoint(
        &self,
        card_id: Uuid,
        step: u32,
        state: &serde_json::Value,
        usage: crate::ledger::Usage,
    ) -> Result<serde_json::Value, HubError>;
    async fn fail_card(&self, card_id: Uuid, reason: &str) -> Result<serde_json::Value, HubError>;
    async fn release_card(
        &self,
        card_id: Uuid,
        reason: &str,
    ) -> Result<serde_json::Value, HubError>;
    async fn spawn_child_card(
        &self,
        parent_card_id: Uuid,
        key: &str,
        title: &str,
        modality: &str,
        inputs: &str,
        acceptance: &str,
        required_capabilities: serde_json::Value,
    ) -> Result<SpawnedCard, HubError>;
    async fn wait_on_child(
        &self,
        card_id: Uuid,
        child_card_id: Uuid,
    ) -> Result<serde_json::Value, HubError>;
    async fn mcp_server_config(&self, server_id: Uuid) -> Result<McpServerConfig, HubError>;
    async fn check_in(
        &self,
        caps: &Capabilities,
        region: Option<&str>,
    ) -> Result<serde_json::Value, HubError>;
    async fn heartbeat(&self, prev_rtt_ms: Option<u64>) -> Result<(String, u64), HubError>;
    async fn check_out(&self) -> Result<String, HubError>;
    async fn get_schedule(&self) -> Result<Option<serde_json::Value>, HubError>;
    /// Explicit escape hatch for existing community-only adapters. Local hubs return None.
    fn community_client(&self) -> Option<&HubClient> {
        None
    }
    /// Runtime receipts belong to the same data plane as the project, not account-wide memory.
    async fn post_activity(
        &self,
        event_type: &str,
        body: &str,
        payload: serde_json::Value,
    ) -> Result<(), HubError>;
}

#[async_trait::async_trait]
impl Hub for SupabaseHub {
    async fn claim_card(&self) -> Result<Claim, HubError> {
        HubClient::claim_card(self).await
    }
    async fn complete_card(
        &self,
        card_id: Uuid,
        content: &str,
        model_id: Option<&str>,
        usage: crate::ledger::Usage,
    ) -> Result<Completion, HubError> {
        HubClient::complete_card(self, card_id, content, model_id, usage).await
    }
    async fn checkpoint(
        &self,
        card_id: Uuid,
        step: u32,
        state: &serde_json::Value,
        usage: crate::ledger::Usage,
    ) -> Result<serde_json::Value, HubError> {
        HubClient::checkpoint(self, card_id, step, state, usage).await
    }
    async fn fail_card(&self, card_id: Uuid, reason: &str) -> Result<serde_json::Value, HubError> {
        HubClient::fail_card(self, card_id, reason).await
    }
    async fn release_card(
        &self,
        card_id: Uuid,
        reason: &str,
    ) -> Result<serde_json::Value, HubError> {
        HubClient::release_card(self, card_id, reason).await
    }
    async fn spawn_child_card(
        &self,
        parent_card_id: Uuid,
        key: &str,
        title: &str,
        modality: &str,
        inputs: &str,
        acceptance: &str,
        required_capabilities: serde_json::Value,
    ) -> Result<SpawnedCard, HubError> {
        HubClient::spawn_child_card(
            self,
            parent_card_id,
            key,
            title,
            modality,
            inputs,
            acceptance,
            required_capabilities,
        )
        .await
    }
    async fn wait_on_child(
        &self,
        card_id: Uuid,
        child_card_id: Uuid,
    ) -> Result<serde_json::Value, HubError> {
        HubClient::wait_on_child(self, card_id, child_card_id).await
    }
    async fn mcp_server_config(&self, server_id: Uuid) -> Result<McpServerConfig, HubError> {
        HubClient::mcp_server_config(self, server_id).await
    }
    async fn check_in(
        &self,
        caps: &Capabilities,
        region: Option<&str>,
    ) -> Result<serde_json::Value, HubError> {
        HubClient::check_in(self, caps, region).await
    }
    async fn heartbeat(&self, prev_rtt_ms: Option<u64>) -> Result<(String, u64), HubError> {
        HubClient::heartbeat(self, prev_rtt_ms).await
    }
    async fn check_out(&self) -> Result<String, HubError> {
        HubClient::check_out(self).await
    }
    async fn get_schedule(&self) -> Result<Option<serde_json::Value>, HubError> {
        HubClient::get_schedule(self).await
    }
    fn community_client(&self) -> Option<&HubClient> {
        Some(self)
    }
    async fn post_activity(
        &self,
        event_type: &str,
        body: &str,
        payload: serde_json::Value,
    ) -> Result<(), HubError> {
        self.personal_channel_post_node_event(event_type, body, payload)
            .await
            .map(|_| ())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureRequest {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub status: String,
    pub created_at: String,
}

/// Raw response from the `generate-image` Edge Function -- `crates/ohhive-ffi/src/media.rs`
/// decodes `image_base64` into a temp PNG file before handing anything to Swift. BYOK-only
/// (2026-09-13): no charge/balance fields anymore, since the member's own OpenAI key is billed
/// directly by OpenAI -- nothing is ever charged to Honey for this path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedImageHosted {
    pub image_base64: String,
}

/// One turn of chat sent to/received from the `interview` Edge Function's node-key path
/// (2026-09-13, ADR-018 decision 5 follow-on -- wires a member's own BYOK provider into the
/// Swift app's Chat tab). Same shape as that function's `Msg` type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

/// Reply from a BYOK chat turn. Only what the Chat tab actually shows -- `brain` (e.g. "your
/// anthropic key") is a nice-to-have provenance hint; charged/balance/usage aren't surfaced here
/// since this path never charges Honey (see `interview_chat`'s doc comment).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatReply {
    pub reply: String,
    #[serde(default)]
    pub brain: Option<String>,
}

/// Wire shape for `code_brain_turn` -- deliberately mirrors `crate::coder::BrainMessage`
/// field-for-field (role/content/tool_calls/tool_call_id) since `CloudBrain` translates 1:1, but
/// stays a separate type so this module never depends on `coder` (feature-gated differently, and
/// hub.rs should stay usable without pulling in the whole coding-agent module tree).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeBrainMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<CodeBrainToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeBrainToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Same OpenAI-function-calling shape as `crate::coder::ToolSpec` -- see that type's doc for why
/// this shape is close enough to universal that Anthropic/OpenAI both accept it with only minor
/// reshaping inside the Edge Function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeBrainTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// What the Edge Function decided the cloud brain should do next. `tokens_in`/`tokens_out` are
/// carried through for a future usage/receipt trail even though nothing charges Honey for
/// coding-agent turns today (ADR-024's local-execution-only gate) -- see `worker.rs::run_code_card`'s
/// doc for why this path never meters.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodeBrainTurnResult {
    Text {
        text: String,
        #[serde(default)]
        tokens_in: u64,
        #[serde(default)]
        tokens_out: u64,
    },
    ToolCalls {
        calls: Vec<CodeBrainToolCall>,
        #[serde(default)]
        tokens_in: u64,
        #[serde(default)]
        tokens_out: u64,
    },
}

/// One BYOK provider's key status (`hive.member_keys` row, minus the actual secret). Both front
/// doors -- the web app's `hive_member_keys_status` (auth.uid()) and this node-key-resolved
/// `hive_node_member_key_status` -- return the identical shape, since both delegate to the same
/// `hive.member_keys_status_for` core (migration 20260913120000_node_key_byok_management.sql,
/// 2026-09-13: Jack, "they need to be able to operate independent of each other" -- the Swift app
/// never holds a member Supabase session, only this node's own key, so it needed its own
/// authenticated path to the same read/write surface the web Settings page already had).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberKeyInfo {
    #[serde(default)]
    pub last4: String,
    #[serde(default)]
    pub since: String,
    #[serde(default)]
    pub preferred_model: Option<String>,
}

/// Fixed to the three known providers (matching `hive.member_key_set`'s own
/// `p_provider not in ('anthropic','openai','nous')` check) rather than a dynamic map -- simpler
/// for `crate::ffi`/Swift to consume than a `HashMap`, and the provider set changing is rare
/// enough that a new one would need a code change here anyway.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemberKeysStatus {
    #[serde(default)]
    pub anthropic: Option<MemberKeyInfo>,
    #[serde(default)]
    pub openai: Option<MemberKeyInfo>,
    #[serde(default)]
    pub nous: Option<MemberKeyInfo>,
}

/// The member's persistent chat memory (2026-09-13, Hermes-agent survey -- see
/// `supabase/migrations/20260913010000_chat_memory.sql`). Read-only from this node: only the
/// `interview` Edge Function's background pass writes it, on the member's own BYOK key -- see
/// that migration's header for why on-device chat doesn't author memory itself yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMemory {
    #[serde(default)]
    pub memory_md: String,
    #[serde(default)]
    pub user_md: String,
}

/// One published release note (#178, 2026-09-13 -- Jack: "when users login after an update there
/// should be release notes"). Read-only from this node: only `hive.release_notes_publish`
/// (admin-only, web Settings) ever writes this table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseNote {
    pub seq: i64,
    pub version: String,
    pub title: String,
    pub body_md: String,
    pub published_at: String,
}

/// One row of the member's Private Fleet channel (2026-09-13, ADR-022 S2 -- "I want to be able to
/// see the receipts, so I think we take Buzz's channel model and run with it"). `node_display` is
/// pre-joined server-side so Swift never needs a second round-trip just to label who posted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPost {
    pub id: Uuid,
    #[serde(default)]
    pub node_id: Option<Uuid>,
    #[serde(default)]
    pub node_display: Option<String>,
    pub author_kind: String,
    pub event_type: String,
    pub body: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub created_at: String,
}

/// A member-configured MCP server (#177, ADR-023), as returned by
/// `hive_member_mcp_server_get_node` (migration `20260913090000_member_mcp_servers.sql`). Fetched
/// fresh right before a card's `mcp_server_id` tool step runs — that RPC re-checks ownership and
/// `enabled` independently of whatever `hive.node_claim_card` already checked at claim time, so a
/// member disabling or deleting a server in between still takes effect. `crate::tools::run_mcp_tool_call`
/// converts this into `crate::mcp::McpServerConfig` (a separate, `hub`-independent type — see that
/// module's doc for why) before handing it to the actual stdio client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: Uuid,
    pub name: String,
    pub transport: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
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

    /// Like `rpc`, but for a Supabase Edge Function (`/functions/v1/<name>`) instead of a
    /// PostgREST RPC (`/rest/v1/rpc/<name>`) -- a different gateway path, needed for calls that
    /// require actual server-side logic (an outbound HTTPS request to a provider) rather than pure
    /// SQL. The target function must be deployed with `verify_jwt: false` and do its own auth --
    /// this node has no member session to present, only the node key, which goes in the body like
    /// any other request field (see `generate_image_hosted` below).
    async fn edge_function<T: for<'de> Deserialize<'de>>(
        &self,
        name: &str,
        body: serde_json::Value,
    ) -> Result<T, HubError> {
        let resp = self
            .http
            .post(format!("{}/functions/v1/{}", self.base, name))
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

    /// Submit a feature request as this node's owning member. The desktop/CLI node has no
    /// member Supabase session (only this node key), so this goes through
    /// `public.hive_feature_request_create_node` (migration 20260912200000), which resolves the
    /// node key to a member server-side -- same shape as every other node-authenticated call
    /// here, just reused for a plain-data write instead of hub-protocol bookkeeping.
    pub async fn submit_feature_request(
        &self,
        title: &str,
        description: &str,
    ) -> Result<FeatureRequest, HubError> {
        self.rpc(
            "hive_feature_request_create_node",
            serde_json::json!({ "p_raw_key": self.node_key, "p_title": title, "p_description": description }),
        )
        .await
    }

    /// One provider's BYOK key status (`hive.member_keys` row), as seen from either front door --
    /// see `MemberKeysStatus`'s doc for why this node-key path exists at all.
    pub async fn member_key_status(&self) -> Result<MemberKeysStatus, HubError> {
        self.rpc(
            "hive_node_member_key_status",
            serde_json::json!({ "p_raw_key": self.node_key }),
        )
        .await
    }

    /// Save (or replace) this node's owning member's key for `provider` ("anthropic" | "openai" |
    /// "nous"). Node-key-resolved twin of the web app's `hive_member_key_set` (2026-09-13, Jack:
    /// "I'd like the picker in the swift app as well ... they need to be able to operate
    /// independent of each other") -- see `hive.member_key_set_for` (migration
    /// 20260913120000_node_key_byok_management.sql) for the shared core both front doors delegate
    /// to. Errors surface the same `key_too_short`/`unknown_provider` messages the web app's RPC
    /// does; `crate::ffi` callers are expected to translate those into friendly text the same way
    /// `send_byok_chat` already does for `no_byo_key`.
    pub async fn member_key_set(&self, provider: &str, key: &str) -> Result<(), HubError> {
        self.rpc::<serde_json::Value>(
            "hive_node_member_key_set",
            serde_json::json!({ "p_raw_key": self.node_key, "p_provider": provider, "p_key": key }),
        )
        .await?;
        Ok(())
    }

    /// Remove this node's owning member's key for `provider`. Returns `false` (not an error) if
    /// there was no key on file for that provider, matching the web app's `hive_member_key_remove`.
    pub async fn member_key_remove(&self, provider: &str) -> Result<bool, HubError> {
        self.rpc(
            "hive_node_member_key_remove",
            serde_json::json!({ "p_raw_key": self.node_key, "p_provider": provider }),
        )
        .await
    }

    /// Set (or, with an empty string, clear back to the function's default) which model
    /// `provider`'s key should use -- e.g. a specific Claude/GPT model instead of whatever
    /// `interview`'s `INTERVIEW_MODEL`/etc. secrets default to. Fails with `no_key_for_provider`
    /// if there's no key on file yet for that provider (nothing to attach a model preference to).
    pub async fn member_key_set_model(&self, provider: &str, model: &str) -> Result<(), HubError> {
        self.rpc::<serde_json::Value>(
            "hive_node_member_key_set_model",
            serde_json::json!({ "p_raw_key": self.node_key, "p_provider": provider, "p_model": model }),
        )
        .await?;
        Ok(())
    }

    /// Plain BYOK chat (2026-09-13, Jack: "get the BYOK to swift") -- routes a conversation
    /// through the member's own Claude/OpenAI/Nous key via the `interview` Edge Function's
    /// node-key path (migration-free; that function's auth already fell back to a raw node key
    /// when no member JWT is present, see its 2026-09-13 header note). Always `mode: "chat"` --
    /// the Swift Chat tab is a small utility panel (`ChatEngine.swift`), not a project-building
    /// surface. Returns `no_byo_key` (surfaced as a `HubError::Rejected`) if the member hasn't
    /// added a key in Settings -- the caller's on-device Apple Intelligence path is the fallback,
    /// not a hub-funded model, so there's no retry-without-a-key behavior here to build.
    pub async fn interview_chat(&self, messages: &[ChatTurn]) -> Result<ChatReply, HubError> {
        self.edge_function(
            "interview",
            serde_json::json!({ "raw_key": self.node_key, "messages": messages, "mode": "chat" }),
        )
        .await
    }

    /// One turn of `crate::coder`'s cloud brain (#186, ADR-024 decision 3): hand the running
    /// coding-agent conversation to the `code-brain-turn` Edge Function, which calls
    /// `provider`/`model` on the member's own BYOK key and reports back what to do next. This is
    /// a pure "what should happen next" oracle -- the actual tools (read_file/write_file/
    /// list_dir/run_command) never run here or in the Edge Function, only on this node, same as
    /// `LocalBrain`. See `crate::coder::CloudBrain`, the only caller, for the translation
    /// to/from `crate::coder`'s `BrainMessage`/`ToolSpec`/`BrainTurn` vocabulary -- this method
    /// and its wire types intentionally know nothing about that module so `hub.rs` stays a pure
    /// transport layer (same separation `MemberKeyInfo`/`ByokKeyInfo` already use between core
    /// and FFI).
    pub async fn code_brain_turn(
        &self,
        provider: &str,
        model: Option<&str>,
        messages: &[CodeBrainMessage],
        tools: &[CodeBrainTool],
    ) -> Result<CodeBrainTurnResult, HubError> {
        self.edge_function(
            "code-brain-turn",
            serde_json::json!({
                "raw_key": self.node_key,
                "provider": provider,
                "model": model,
                "messages": messages,
                "tools": tools,
            }),
        )
        .await
    }

    /// Read this node's owning member's persistent chat memory (2026-09-13, see `ChatMemory`'s
    /// doc comment). Plain PostgREST RPC, not an Edge Function -- `hive_chat_memory_get_node`
    /// (migration 20260913010000) does the node-key-to-member resolution and read in one
    /// security-definer call, same shape as `hive_bug_report_create_node`. On-device chat
    /// (`ChatEngine.swift`'s `.systemOnDevice` path) calls this once per session to pick up
    /// whatever the member's BYOK sessions have taught the assistant, even though on-device chat
    /// never writes to it itself.
    pub async fn chat_memory_get(&self) -> Result<ChatMemory, HubError> {
        self.rpc(
            "hive_chat_memory_get_node",
            serde_json::json!({ "p_raw_key": self.node_key }),
        )
        .await
    }

    /// Read whatever release notes this node's owning member hasn't seen yet (#178). Same
    /// node-key-to-member resolution as `chat_memory_get`, via `hive_release_notes_unseen_node`
    /// (migration 20260913080000). Oldest-unseen-first, matching that RPC's ordering.
    pub async fn release_notes_unseen(&self) -> Result<Vec<ReleaseNote>, HubError> {
        self.rpc(
            "hive_release_notes_unseen_node",
            serde_json::json!({ "p_raw_key": self.node_key }),
        )
        .await
    }

    /// Acknowledge every release note published so far, moving this member's watermark forward
    /// server-side (`hive_release_notes_mark_seen_node`) -- so the one-time "what's new" dialog
    /// doesn't come back next launch, on this machine or any other this member signs into.
    pub async fn release_notes_mark_seen(&self) -> Result<(), HubError> {
        self.rpc::<serde_json::Value>(
            "hive_release_notes_mark_seen_node",
            serde_json::json!({ "p_raw_key": self.node_key }),
        )
        .await?;
        Ok(())
    }

    /// Read the member's Private Fleet channel (2026-09-13, #184 -- Swift catching up to the web
    /// UI's #183). `node_id: None` mirrors the web page's "All machines" option; `Some(id)` filters
    /// to one paired machine -- same fleet-wide table either way (ADR-022 S2 decision 2), just a
    /// query parameter, per `hive_personal_channel_list_node` (migration 20260913060000).
    pub async fn channel_list(
        &self,
        node_id: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<ChannelPost>, HubError> {
        self.rpc(
            "hive_personal_channel_list_node",
            serde_json::json!({ "p_raw_key": self.node_key, "p_node_id": node_id, "p_limit": limit }),
        )
        .await
    }

    /// Post a member-authored message into the Private Fleet channel from this machine -- the
    /// same action as typing into the web page's input box (`hive_personal_channel_post_node`,
    /// migration 20260913060000).
    pub async fn channel_post(&self, body: &str) -> Result<ChannelPost, HubError> {
        self.rpc(
            "hive_personal_channel_post_node",
            serde_json::json!({ "p_raw_key": self.node_key, "p_body": body }),
        )
        .await
    }

    /// Post one node-authored *activity* event into the Private Fleet channel (ADR-024 decision
    /// 6, via `hive_personal_channel_post_node_event`, migration
    /// `20260913110000_channel_post_node_event.sql`) — distinct from [`Self::channel_post`],
    /// which posts as `author_kind = 'member'` ("the member typed this from the Mac app"). A
    /// coding session's own progress ("started", "ran `cargo test`", "finished") is fleet
    /// *activity* a node observed about itself, not something a human typed — same distinction
    /// that migration's own header comment draws. `event_type` is a short machine-readable tag
    /// (e.g. `"code_session_started"`, `"code_session_tool"`, `"code_session_finished"`);
    /// `payload` is optional structured detail a future UI could render specially. Used today
    /// only by [`crate::coder::run_session`]'s progress posting.
    pub async fn personal_channel_post_node_event(
        &self,
        event_type: &str,
        body: &str,
        payload: serde_json::Value,
    ) -> Result<ChannelPost, HubError> {
        self.rpc(
            "hive_personal_channel_post_node_event",
            serde_json::json!({
                "p_raw_key": self.node_key, "p_event_type": event_type, "p_body": body, "p_payload": payload,
            }),
        )
        .await
    }

    /// Fetch the command/args/env for a member-configured MCP server, right before spawning it
    /// for a claimed card's `required_capabilities.mcp_server_id` (#177, ADR-023). Same
    /// node-key-to-member resolution as `chat_memory_get`/`channel_list`, via
    /// `hive_member_mcp_server_get_node` (migration `20260913090000_member_mcp_servers.sql`),
    /// which raises (not an empty/null result) if the server doesn't exist, isn't owned by this
    /// node's member, or has been disabled — this call is the *second* independent check of
    /// ownership+enabled, on top of the one `hive.node_claim_card` already made when the card was
    /// leased; the caller (`crate::tools::run_mcp_tool_call`) should treat any error here as a
    /// hard tool-call failure, never silently skip the tool.
    pub async fn mcp_server_config(&self, server_id: Uuid) -> Result<McpServerConfig, HubError> {
        self.rpc(
            "hive_member_mcp_server_get_node",
            serde_json::json!({ "p_raw_key": self.node_key, "p_server_id": server_id }),
        )
        .await
    }

    /// Submit a bug report as this node's owning member (2026-09-13, mirrors
    /// `submit_feature_request` above). Resolves the node key to a member server-side via
    /// `public.hive_bug_report_create_node` (migration 20260913000000) -- same reasoning as
    /// feature requests, this Mac never holds a member Supabase session. Attachments and the
    /// list/comments/follow UI stay web-only (see that migration's header).
    pub async fn submit_bug_report(
        &self,
        title: &str,
        description: &str,
        anonymous: bool,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_bug_report_create_node",
            serde_json::json!({ "p_raw_key": self.node_key, "p_title": title, "p_description": description, "p_anonymous": anonymous }),
        )
        .await
    }

    /// Hosted image generation (M8 follow-on, tasks #125-128): OpenAI's image API, using this
    /// node's owning member's own OpenAI key (BYOK-only as of 2026-09-13 -- Jack: "running it
    /// hosted will guarantee a spend, I think we've got to make it so that it's bring your own
    /// key"). No Hive/Honey cost either way -- OpenAI bills the member directly. A member with no
    /// OpenAI key on file gets a `no_byo_key` rejection from the Edge Function; `ComfyUiBackend`
    /// (crates/ohhive-core/src/backend/comfyui.rs) is the free, no-OpenAI-account alternative.
    /// Goes through the `generate-image` Edge Function (not a plain RPC) since it needs to make an
    /// outbound call to OpenAI with the member's key, which only server-side code should ever see.
    pub async fn generate_image_hosted(
        &self,
        prompt: &str,
        negative_prompt: Option<&str>,
    ) -> Result<GeneratedImageHosted, HubError> {
        self.edge_function(
            "generate-image",
            serde_json::json!({ "raw_key": self.node_key, "prompt": prompt, "negative_prompt": negative_prompt }),
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

    /// `prev_rtt_ms` is this call's own round-trip time as measured on the PREVIOUS heartbeat --
    /// reported now so the hub has a number for "how fast does this node reach the Hive" without
    /// a second round trip just to say so (one heartbeat interval stale, fine for a slow-changing
    /// network metric). Returns the hub's timestamp plus this call's own elapsed time, which the
    /// caller feeds back in as `prev_rtt_ms` next tick.
    pub async fn heartbeat(&self, prev_rtt_ms: Option<u64>) -> Result<(String, u64), HubError> {
        let start = std::time::Instant::now();
        let ts: String = self
            .rpc(
                "hive_node_heartbeat",
                serde_json::json!({ "raw_key": self.node_key, "p_rtt_ms": prev_rtt_ms }),
            )
            .await?;
        Ok((ts, start.elapsed().as_millis() as u64))
    }

    /// Returns the resulting presence: "checked_out" or "draining" (lease held).
    pub async fn check_out(&self) -> Result<String, HubError> {
        self.rpc(
            "hive_node_checkout",
            serde_json::json!({ "raw_key": self.node_key }),
        )
        .await
    }

    /// This node's scheduled check-in/out window, if the owning member has set one
    /// (migration 20260912270000). `None` means no schedule -- always eligible, exactly
    /// today's behavior. Cheap, read-only; the `--stay` loop refetches it every tick so an
    /// edit made on the web takes effect within one heartbeat interval.
    pub async fn get_schedule(&self) -> Result<Option<serde_json::Value>, HubError> {
        self.rpc(
            "hive_node_schedule_get",
            serde_json::json!({ "p_raw_key": self.node_key }),
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

    /// `prev_rtt_ms` mirrors `heartbeat()` above -- this server's own round-trip time to the hub,
    /// as measured on the PREVIOUS server_heartbeat call, reported now (one interval stale).
    /// Returns the hub's response plus this call's own elapsed time for the caller to feed back
    /// in as `prev_rtt_ms` next tick.
    pub async fn server_heartbeat(
        &self,
        storage_used_bytes: u64,
        connections: u32,
        prev_rtt_ms: Option<u64>,
    ) -> Result<(serde_json::Value, u64), HubError> {
        let start = std::time::Instant::now();
        let res: serde_json::Value = self
            .rpc(
                "hive_server_heartbeat",
                serde_json::json!({ "raw_key": self.node_key, "p_storage_used_bytes": storage_used_bytes, "p_connections": connections, "p_rtt_ms": prev_rtt_ms }),
            )
            .await?;
        Ok((res, start.elapsed().as_millis() as u64))
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimedProject {
    pub id: Uuid,
    pub title: String,
    pub goal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRecord {
    pub step: u32,
    pub blob_hash: String,
    pub usage: crate::ledger::Usage,
    pub state: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
