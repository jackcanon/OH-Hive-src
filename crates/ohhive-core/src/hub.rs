//! Hub client — how a node talks to the source of record (ADR-001) without
//! ever holding a Supabase user JWT (ADR-004). Auth is a *node key* minted by
//! the owning member (migration 0002); every call is a PostgREST RPC.
//!
//! Endpoints are the `public.hive_*` wrappers until schema `hive` is exposed
//! in the project's API settings, after which `Content-Profile: hive` and the
//! unprefixed names work too.

use crate::capability::{Capabilities, ToolsLevel};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;
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

/// Shared HTTP client policy for every Supabase-facing client in this module (`SupabaseHub`/
/// `HubClient`, `MemberClient`, `Pairing`): reqwest sets no timeout by default, so a stalled
/// server previously left `claim_card`/`heartbeat`/`checkpoint`/etc. waiting forever -- nothing
/// here bounded that wait (Sif's efficiency audit, finding 6, 2026-09-15). A per-request
/// `.timeout(..)` overrides this default where a call needs a different allowance (heartbeat
/// tighter, an Edge Function proxying an actual model/image call looser, an artifact transfer
/// looser still).
const HUB_DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// Heartbeat must never be allowed to hang as long as an ordinary RPC -- `worker.rs`'s
/// `run_forever` now polls `dispatch_loop` and the heartbeat ticker concurrently (finding 1), but
/// a heartbeat call that never resolves would still stall the RTT feedback loop and the next
/// tick's `MissedTickBehavior::Delay` cadence, so it gets the tightest bound in this module.
const HUB_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(8);
/// Edge Functions proxy an actual provider call (chat, the coder brain turn, image generation) --
/// real model/generation latency, not a database round trip, so they get their own longer
/// allowance rather than sharing the plain-RPC default.
const HUB_EDGE_FUNCTION_TIMEOUT: Duration = Duration::from_secs(180);
/// Artifact upload/download moves real file bytes over the wire, not a small JSON reply.
const HUB_ARTIFACT_TIMEOUT: Duration = Duration::from_secs(120);
/// Bound on a plain RPC/Edge-Function JSON reply -- generous for any legitimate response shape
/// this module deserializes, far below "however much a server feels like sending."
const HUB_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
/// Full-schema exports exceed ordinary RPC payloads, but must remain bounded.
const HUB_MAX_BACKUP_BYTES: usize = 64 * 1024 * 1024;
/// Bound on artifact bytes fetched from a regional server -- large binary content is expected
/// here, unlike the JSON-reply bound above, but it still isn't unbounded.
const HUB_MAX_ARTIFACT_BYTES: usize = 512 * 1024 * 1024;
/// An error body gets folded into `HubError::Rejected`'s message, which can end up in logs or a
/// UI toast -- cap how much of a bad/oversized error response gets carried along.
const HUB_MAX_ERROR_EXCERPT_BYTES: usize = 2048;

/// Every `reqwest::Client` this module builds shares this default timeout; call sites that need
/// a different allowance override it per-request with `.timeout(..)`.
fn hub_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(HUB_DEFAULT_TIMEOUT)
        .build()
        .unwrap_or_default()
}

/// Reads a response body as a stream, erroring out instead of buffering without limit if the
/// server sends more than `max_bytes`. Every full-body read in this module goes through this
/// rather than a bare `.text()`/`.bytes()` call (finding 6, Sif's efficiency audit, 2026-09-15).
async fn read_body_bounded(resp: reqwest::Response, max_bytes: usize) -> Result<Vec<u8>, HubError> {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| HubError::Transport(e.to_string()))?;
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(HubError::Transport(format!(
                "response body exceeded {max_bytes} byte bound"
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Truncates an error body to a bounded excerpt so a malformed or oversized error response can't
/// make a `HubError::Rejected` message unbounded too.
fn bounded_error_excerpt(text: &str) -> String {
    if text.len() <= HUB_MAX_ERROR_EXCERPT_BYTES {
        text.to_string()
    } else {
        let mut cut = HUB_MAX_ERROR_EXCERPT_BYTES;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        format!(
            "{}... [truncated, {} bytes total]",
            &text[..cut],
            text.len()
        )
    }
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
// `spawn_child_card` takes eight arguments because it mirrors the `node_spawn_child_card` RPC's
// parameter list one-for-one, and that correspondence is worth more than the lint: an args struct
// here would have to be kept in sync with the SQL signature by hand, and a mismatch would be a
// silent wrong-column bug rather than a compile error.
#[allow(clippy::too_many_arguments)]
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

/// Cloud turn, including actual token usage and the model selected by the server.
/// Older servers can omit model_id; callers must preserve that uncertainty.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodeBrainTurnResult {
    Text {
        text: String,
        #[serde(default)]
        tokens_in: u64,
        #[serde(default)]
        tokens_out: u64,
        #[serde(default)]
        model_id: Option<String>,
    },
    ToolCalls {
        calls: Vec<CodeBrainToolCall>,
        #[serde(default)]
        tokens_in: u64,
        #[serde(default)]
        tokens_out: u64,
        #[serde(default)]
        model_id: Option<String>,
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

/// One live model entry from a BYOK provider's own catalog (2026-09-15, Jack: the chat
/// composer's model picker was "not actually loading with models" -- `ProviderModelPicker.swift`
/// shipped 9/13 with only a "Default"/free-text "Custom..." stage 2 since there was no live
/// per-provider model list yet). `label`, when present, is a friendlier display name -- only
/// Anthropic's `/v1/models` returns one; OpenAI's and Nous's don't, so `id` doubles as the label
/// there (see the `interview` Edge Function's `listOpenAICompatibleModels`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByokModel {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
}

/// Wire shape of the `interview` Edge Function's `mode: "list_models"` success response --
/// `{ models: [...] }`, not a bare array, so `list_byok_models` has something to deserialize
/// into before unwrapping it for callers.
#[derive(Debug, Clone, Deserialize)]
struct ByokModelsResponse {
    models: Vec<ByokModel>,
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

/// ADR-030: one row of `hive_code_session_projects_node` -- just enough to resolve a
/// `--project <title>` flag to an id, same shape `hive_code_session_projects` returns the web
/// app's own picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeProjectSummary {
    pub id: Uuid,
    pub title: String,
}

/// ADR-030: `hive_code_session_create_node`'s result -- just enough for `hive card submit` to
/// print the new card's id and hand it straight to `hive card status`/`await`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardSubmitResult {
    pub card_id: Uuid,
    pub project_id: Uuid,
}

/// ADR-030: `hive_code_session_status_node`'s result -- `status` is one of `hive.cards`'
/// existing enum values (`ready`/`running`/`blocked`/`waiting_on_child`/`review`/`done`, plus
/// `suggested`, unreachable here since a submitted card starts at `ready`); `latest_output` is
/// `None` until the claiming node reports something via `complete_card`/`checkpoint`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardStatus {
    pub card_id: Uuid,
    pub project_id: Uuid,
    pub status: String,
    pub title: String,
    pub key: String,
    pub created_at: String,
    #[serde(default)]
    pub latest_output: Option<String>,
    /// When the newest output row was written. `None` when there is no output yet.
    #[serde(default)]
    pub latest_output_at: Option<String>,
    /// The model that produced the newest output, as the node reported it.
    ///
    /// This and `usage` come from the SAME output row by construction -- migration
    /// 20260916060000 rewrote this RPC to use one lateral join precisely so that a card's model
    /// and its token counts cannot come from different rows and be silently mismatched.
    #[serde(default)]
    pub model_id: Option<String>,
    /// Metered usage from the latest output. Failed outputs may contain `{}`,
    /// meaning unavailable rather than a measured zero.
    #[serde(default, deserialize_with = "optional_output_usage")]
    pub usage: Option<crate::Usage>,
}

fn optional_output_usage<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<crate::Usage>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(serde_json::Value::Object(ref object)) if object.is_empty() => Ok(None),
        Some(value) => serde_json::from_value(value)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
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
            http: hub_http_client(),
        }
    }

    async fn rpc<T: for<'de> Deserialize<'de>>(
        &self,
        name: &str,
        body: serde_json::Value,
    ) -> Result<T, HubError> {
        self.rpc_with_timeout(name, body, HUB_DEFAULT_TIMEOUT).await
    }

    /// Same as `rpc`, but with an explicit per-call timeout override -- used by `heartbeat`,
    /// which needs a tighter bound than the rest of this module's plain RPCs (findings 1 and 6,
    /// Sif's efficiency audit, 2026-09-15).
    async fn rpc_with_timeout<T: for<'de> Deserialize<'de>>(
        &self,
        name: &str,
        body: serde_json::Value,
        timeout: Duration,
    ) -> Result<T, HubError> {
        self.rpc_with_limits(name, body, timeout, HUB_MAX_RESPONSE_BYTES)
            .await
    }

    async fn rpc_with_limits<T: for<'de> Deserialize<'de>>(
        &self,
        name: &str,
        body: serde_json::Value,
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<T, HubError> {
        let resp = self
            .http
            .post(format!("{}/rest/v1/rpc/{}", self.base, name))
            .timeout(timeout)
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.anon_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| HubError::Transport(e.to_string()))?;
        let status = resp.status();
        let bytes = read_body_bounded(resp, max_bytes).await?;
        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if text.contains("invalid_or_revoked_node_key") {
                return Err(HubError::BadKey);
            }
            return Err(HubError::Rejected(format!(
                "{status}: {}",
                bounded_error_excerpt(&text)
            )));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            HubError::Rejected(format!(
                "bad response: {e}: {}",
                bounded_error_excerpt(&text)
            ))
        })
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
            .timeout(HUB_EDGE_FUNCTION_TIMEOUT)
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.anon_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| HubError::Transport(e.to_string()))?;
        let status = resp.status();
        let bytes = read_body_bounded(resp, HUB_MAX_RESPONSE_BYTES).await?;
        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if text.contains("invalid_or_revoked_node_key") {
                return Err(HubError::BadKey);
            }
            return Err(HubError::Rejected(format!(
                "{status}: {}",
                bounded_error_excerpt(&text)
            )));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            HubError::Rejected(format!(
                "bad response: {e}: {}",
                bounded_error_excerpt(&text)
            ))
        })
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

    /// Additive twin of `interview_chat` (2026-09-13, Jack: the chat composer's two-stage
    /// provider-then-model picker, matching Cowork/Codex/Hermes) -- same request, plus an
    /// explicit `provider`/`model` the member chose in the picker, forwarded to the `interview`
    /// Edge Function's own `provider`/`model` override (deployed v17, see that function's header).
    /// `interview_chat` above is untouched and still used for the no-explicit-choice/auto path
    /// (today: whichever configured key comes first in priority order) -- this method exists so a
    /// member who picked "OpenAI" in the UI doesn't silently get an Anthropic reply just because
    /// an Anthropic key also happens to be on file. `model` empty/`None` means "use that
    /// provider's saved `preferred_model`, or the function's default if none is saved" -- the Edge
    /// Function already implements exactly that fallback.
    pub async fn interview_chat_with(
        &self,
        provider: Option<&str>,
        model: Option<&str>,
        messages: &[ChatTurn],
    ) -> Result<ChatReply, HubError> {
        let mut body = serde_json::json!({
            "raw_key": self.node_key,
            "messages": messages,
            "mode": "chat",
        });
        if let Some(provider) = provider {
            body["provider"] = serde_json::json!(provider);
        }
        if let Some(model) = model {
            body["model"] = serde_json::json!(model);
        }
        self.edge_function("interview", body).await
    }

    /// Live model catalog for one BYOK provider (2026-09-15, see `ByokModel`'s doc for the "not
    /// actually loading with models" report this answers). Routes through the same `interview`
    /// Edge Function as `interview_chat`/`interview_chat_with` -- its `mode: "list_models"`
    /// branch resolves this node's owning member's stored key for `provider` and calls that
    /// provider's own models endpoint with it, server-side, the same way it calls the chat
    /// endpoint for an actual turn. `provider` is "anthropic" | "openai" | "nous". Returns
    /// `HubError::Rejected("409: ...provider_key_not_configured...")` when there's no key on file
    /// for `provider` yet -- callers are expected to translate that the same friendly way
    /// `crate::ffi`'s `send_byok_chat`/`send_byok_chat_with` already do for `no_byo_key`.
    pub async fn list_byok_models(&self, provider: &str) -> Result<Vec<ByokModel>, HubError> {
        let resp: ByokModelsResponse = self
            .edge_function(
                "interview",
                serde_json::json!({
                    "raw_key": self.node_key,
                    "mode": "list_models",
                    "provider": provider,
                }),
            )
            .await?;
        Ok(resp.models)
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

    /// ADR-030: this node's owning member's own `execution_mode = 'local'` projects, for a
    /// `hive card submit --project <title>` picker to resolve a name against without the caller
    /// ever needing to know a project's uuid. Node-authenticated equivalent of what the web
    /// app's own project picker reads for itself via RLS (`hive_code_session_projects`);
    /// `hive_code_session_projects_node` (`docs/proposed-migrations/`, not yet applied) is the
    /// same query, resolving the caller through `hive.node_member_id` instead of `auth.uid()`.
    pub async fn code_session_projects(&self) -> Result<Vec<CodeProjectSummary>, HubError> {
        self.rpc(
            "hive_code_session_projects_node",
            serde_json::json!({ "p_raw_key": self.node_key }),
        )
        .await
    }

    /// ADR-030: create one `code`-modality card the same way the Kanban already does
    /// (`hive_code_session_create`, ADR-024/Sif) -- just authenticated by this node's own key
    /// instead of a member's browser session, so a `hive card submit` run from a terminal (or
    /// Cowork's `device_bash`, the use case that motivated this) needs nothing but `hive pair`.
    /// The deployed `hive_code_session_create_node` delegates
    /// to the exact same `hive.code_session_create_for` core the web RPC now also delegates to
    /// -- one validated insert path, two front doors. `p_request_id`, when given, makes a retry
    /// of the same submission idempotent (`request_id_conflict` on a genuine change, the
    /// existing card's id on an exact repeat) the same way it already does for the web app.
    #[allow(clippy::too_many_arguments)]
    pub async fn code_session_submit(
        &self,
        project_id: Uuid,
        task: &str,
        workspace_path: Option<&str>,
        repo_url: Option<&str>,
        repo_ref: Option<&str>,
        brain: &str,
        model_id: Option<&str>,
        max_turns: u32,
        cloud_consent: bool,
        request_id: Option<Uuid>,
        // ADR-032: opts the new card into `spawn_card`/`wait_for_child`. `false` (the CLI's
        // default) is identical to submitting before ADR-032 existed -- see
        // `CodeSessionSpec::coordinator`'s own doc for what this actually gates.
        coordinator: bool,
        acceptance: &[crate::acceptance::AcceptanceCheck],
        target_node_id: Option<Uuid>,
    ) -> Result<CardSubmitResult, HubError> {
        crate::acceptance::validate(acceptance)
            .map_err(|message| HubError::Rejected(message.into()))?;
        let mut body = serde_json::json!({
            "p_raw_key": self.node_key,
            "p_project_id": project_id,
            "p_task": task,
            "p_workspace_path": workspace_path,
            "p_repo_url": repo_url,
            "p_repo_ref": repo_ref,
            "p_brain": brain,
            "p_model_id": model_id,
            "p_max_turns": max_turns,
            "p_cloud_consent": cloud_consent,
            "p_request_id": request_id,
            "p_coordinator": coordinator,
        });
        // Omit the key entirely for legacy callers: PostgREST selects by argument names.
        if !acceptance.is_empty() || target_node_id.is_some() {
            body["p_acceptance"] =
                serde_json::to_value(acceptance).expect("acceptance checks are serializable");
        }
        if let Some(node) = target_node_id {
            body["p_target_node_id"] = serde_json::json!(node);
        }
        self.rpc("hive_code_session_create_node", body).await
    }

    /// ADR-030: poll one card's status + latest output -- `hive card status`/`hive card await`.
    /// `hive_code_session_status_node` (`docs/proposed-migrations/`, not yet applied) does the
    /// ownership check a raw `hive.cards` read would otherwise get for free from RLS under a
    /// member session (a node key has none), then returns the same shape the Kanban's own card
    /// detail view reads.
    pub async fn code_session_status(&self, card_id: Uuid) -> Result<CardStatus, HubError> {
        self.rpc(
            "hive_code_session_status_node",
            serde_json::json!({ "p_raw_key": self.node_key, "p_card_id": card_id }),
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
            .rpc_with_timeout(
                "hive_node_heartbeat",
                serde_json::json!({ "raw_key": self.node_key, "p_rtt_ms": prev_rtt_ms }),
                HUB_HEARTBEAT_TIMEOUT,
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
        self.artifact_fetch_bounded(hash, HUB_MAX_ARTIFACT_BYTES)
            .await
    }

    /// A caller may impose a smaller per-modality bound; never exceed the global artifact cap.
    pub async fn artifact_fetch_bounded(
        &self,
        hash: &str,
        limit: usize,
    ) -> Result<(Vec<u8>, String), HubError> {
        let limit = limit.min(HUB_MAX_ARTIFACT_BYTES);
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
            match self.http.get(u).timeout(HUB_ARTIFACT_TIMEOUT).send().await {
                Ok(resp) if resp.status().is_success() => {
                    return match read_body_bounded(resp, limit).await {
                        Ok(b) => Ok((b, mime)),
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
                .timeout(HUB_ARTIFACT_TIMEOUT)
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
                    let reply_bytes = match read_body_bounded(resp, HUB_MAX_RESPONSE_BYTES).await {
                        Ok(b) => b,
                        Err(e) => {
                            last_err = e.to_string();
                            continue;
                        }
                    };
                    return serde_json::from_slice(&reply_bytes).map_err(|e| {
                        let text = String::from_utf8_lossy(&reply_bytes).into_owned();
                        HubError::Rejected(format!(
                            "bad upload reply: {e}: {}",
                            bounded_error_excerpt(&text)
                        ))
                    });
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
        self.rpc_with_limits(
            "hive_backup_export",
            serde_json::json!({ "raw_key": self.node_key }),
            Duration::from_secs(60),
            HUB_MAX_BACKUP_BYTES,
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
            http: hub_http_client(),
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
            .timeout(HUB_DEFAULT_TIMEOUT)
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {jwt}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| HubError::Transport(e.to_string()))?;
        let status = resp.status();
        let bytes = read_body_bounded(resp, HUB_MAX_RESPONSE_BYTES).await?;
        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            return Err(HubError::Rejected(format!(
                "{status}: {}",
                bounded_error_excerpt(&text)
            )));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            HubError::Rejected(format!(
                "bad response: {e}: {}",
                bounded_error_excerpt(&text)
            ))
        })
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
            http: hub_http_client(),
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
            .timeout(HUB_DEFAULT_TIMEOUT)
            .header("apikey", &self.anon_key)
            .header("Authorization", format!("Bearer {}", self.anon_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| HubError::Transport(e.to_string()))?;
        let status = resp.status();
        let bytes = read_body_bounded(resp, HUB_MAX_RESPONSE_BYTES).await?;
        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            return Err(HubError::Rejected(format!(
                "{status}: {}",
                bounded_error_excerpt(&text)
            )));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            HubError::Rejected(format!(
                "bad response: {e}: {}",
                bounded_error_excerpt(&text)
            ))
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn backup_accepts_large_export_but_ordinary_rpc_stays_bounded() {
        fn fixture() -> (String, std::thread::JoinHandle<()>) {
            use std::io::{Read, Write};
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let handle = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request);
                let body = format!(
                    "{{\"fixture\":\"{}\"}}",
                    "x".repeat(HUB_MAX_RESPONSE_BYTES + 1)
                );
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            });
            (url, handle)
        }
        let (url, server) = fixture();
        let hub = HubClient::new(&url, "fixture", "fixture");
        let result = hub.backup_export().await.unwrap();
        assert!(result["fixture"].as_str().unwrap().len() > HUB_MAX_RESPONSE_BYTES);
        server.join().unwrap();
        let (url, server) = fixture();
        let hub = HubClient::new(&url, "fixture", "fixture");
        assert!(hub
            .replication_plan(1)
            .await
            .unwrap_err()
            .to_string()
            .contains("byte bound"));
        server.join().unwrap();
    }

    /// The fixture below is not invented: it is the exact `jsonb_build_object` that
    /// `public.hive_code_session_status_node` produced for card `24f01b4e` in production on
    /// 2026-09-17, with the report text shortened. Writing this fixture by hand from the struct
    /// would only have proved the struct agrees with itself -- what needs proving is that it
    /// agrees with the deployed SQL, which is where the field names actually live.
    ///
    /// The zeros in `usage` are also real, and they are the bug filed as f0c2b6d8: this card ran
    /// four turns against `mistral-small3.2:24b` and recorded no tokens, because `worker.rs`
    /// hands `Usage::default()` to `complete_card`. The test asserts the zeros rather than
    /// pretending otherwise, so when f0c2b6d8 is fixed this test is the thing that has to be
    /// consciously updated.
    #[test]
    fn card_status_reads_every_field_the_status_rpc_returns() {
        let wire = serde_json::json!({
            "key": "code-1f4b5d89-4ada-486e-8226-424a7a4f1c0c",
            "title": "add a function pub fn add(a: i32, b: i32) -> i32 to src/lib.rs",
            "usage": { "tokens_in": 0, "tokens_out": 0, "compute_seconds": 0.0 },
            "status": "review",
            "card_id": "24f01b4e-7a57-4be8-a41c-f2fbe2e57b21",
            "model_id": "mistral-small3.2:24b",
            "created_at": "2026-09-17T02:37:35.882457+00:00",
            "project_id": "b9e09109-2a01-498e-8704-2295218e093c",
            "latest_output": "I added the `pub fn add` function and ran the tests.",
            "latest_output_at": "2026-09-17T02:38:23.816426+00:00",
        });
        let status: CardStatus = serde_json::from_value(wire).expect("the deployed RPC's shape");
        assert_eq!(status.status, "review");
        assert_eq!(status.model_id.as_deref(), Some("mistral-small3.2:24b"));
        assert_eq!(
            status.latest_output_at.as_deref(),
            Some("2026-09-17T02:38:23.816426+00:00")
        );
        let usage = status
            .usage
            .expect("usage is present, even when it is zeros");
        assert_eq!((usage.tokens_in, usage.tokens_out), (0, 0));
    }

    /// A node or database predating 20260916060000 returns the six-field shape with no
    /// `model_id`/`usage`/`latest_output_at` at all. That must stay readable: `hive card status`
    /// against an older deployment is a support path, not an error.
    #[test]
    fn card_status_still_parses_a_response_predating_the_usage_fields() {
        let wire = serde_json::json!({
            "card_id": "24f01b4e-7a57-4be8-a41c-f2fbe2e57b21",
            "project_id": "b9e09109-2a01-498e-8704-2295218e093c",
            "status": "ready",
            "title": "older card",
            "key": "code-old",
            "created_at": "2026-09-01T00:00:00+00:00",
        });
        let status: CardStatus = serde_json::from_value(wire).expect("the pre-migration shape");
        assert!(status.latest_output.is_none());
        assert!(status.model_id.is_none());
        assert!(status.usage.is_none());
    }
    #[test]
    fn card_status_empty_failure_usage_is_unknown_not_zero() {
        let mut wire = serde_json::json!({
            "card_id":"4a48208e-76bc-4b43-abd1-b83844e5cdfa",
            "project_id":"b9e09109-2a01-498e-8704-2295218e093c",
            "status":"blocked", "title":"fixture", "key":"fixture",
            "created_at":"2026-09-17T18:26:16Z", "usage":{},
            "latest_output":"FAILED: host check failed"
        });
        let status: CardStatus = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(status.status, "blocked");
        assert!(status.usage.is_none());
        wire["usage"] = serde_json::json!({"tokens_in":12});
        assert!(serde_json::from_value::<CardStatus>(wire.clone()).is_err());
        wire["usage"] = serde_json::json!({"tokens_in":12,"tokens_out":3,"compute_seconds":0.0});
        assert_eq!(
            serde_json::from_value::<CardStatus>(wire)
                .unwrap()
                .usage
                .unwrap()
                .tokens_in,
            12
        );
    }
}
