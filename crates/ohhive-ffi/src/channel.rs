//! FFI wrapper for the Private Fleet channel (2026-09-13, ADR-022 S2, #184 -- Swift catching up
//! to the web app's #183). Same node-key-only constraint as `chat.rs`/`feedback.rs`: this app
//! never holds a member Supabase session, so both calls go through the node-key-authenticated
//! `hive_personal_channel_list_node`/`hive_personal_channel_post_node` RPCs (migration
//! 20260913060000), which resolve the owning member from the paired device key server-side.
//!
//! IDs cross the FFI boundary as plain strings (matching every other record in this crate --
//! `HiveSnapshot`/`ChatMemory`/etc. do the same rather than exposing `Uuid` to Swift), and
//! `payload` is passed through as a raw JSON string for the same reason `HiveSnapshot.summary_json`
//! is: it's an open-shaped jsonb document not worth modeling field-by-field yet.

use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::hub::{ChannelPost as HubChannelPost, HubClient};
use hive_core::nodeconfig;
use std::sync::Arc;
use uuid::Uuid;

#[derive(uniffi::Record, Clone)]
pub struct ChannelPost {
    pub id: String,
    pub node_id: Option<String>,
    /// The paired machine's display name, pre-joined server-side -- `None` for a member-authored
    /// message, which has no `node_id` to label.
    pub node_display: Option<String>,
    /// "member" | "node" | "assistant" (reserved).
    pub author_kind: String,
    /// "message" for anything a member typed; a lifecycle event name
    /// ("card_claimed"/"card_completed"/"node_checkin"/"server_online"/...) for automatic posts.
    pub event_type: String,
    pub body: String,
    /// Raw JSON, e.g. `{"card_id": "...", "project_id": "..."}` -- `"{}"` when empty.
    pub payload_json: String,
    pub created_at: String,
}

impl From<HubChannelPost> for ChannelPost {
    fn from(p: HubChannelPost) -> Self {
        Self {
            id: p.id.to_string(),
            node_id: p.node_id.map(|id| id.to_string()),
            node_display: p.node_display,
            author_kind: p.author_kind,
            event_type: p.event_type,
            body: p.body,
            payload_json: p.payload.to_string(),
            created_at: p.created_at,
        }
    }
}

#[uniffi::export]
impl HiveNode {
    /// Reads the member's Private Fleet channel. `node_id: None` (or empty) is the "all machines"
    /// view; pass one of the ids from `HiveSnapshot`/the pairing list to filter to a single
    /// machine -- same fleet-wide table either way (ADR-022 S2), just a query parameter. Errors
    /// (unpaired, hub unreachable, bad node id) are returned rather than swallowed, since the
    /// Private Fleet view's whole point is to show real state, not go quietly blank.
    pub async fn channel_list(
        self: Arc<Self>,
        node_id: Option<String>,
        limit: u32,
    ) -> Result<Vec<ChannelPost>, HiveError> {
        let cfg = nodeconfig::load().map_err(HiveError::from)?;
        let key = cfg
            .node_key
            .clone()
            .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
        let nid = match node_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => Some(
                Uuid::parse_str(s)
                    .map_err(|_| HiveError::Failed("not a valid machine id".into()))?,
            ),
            None => None,
        };
        let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
        let posts = RUNTIME
            .spawn(async move { hub.channel_list(nid, limit.max(1) as i64).await })
            .await
            .map_err(|e| HiveError::Failed(format!("channel_list task panicked: {e}")))?
            .map_err(HiveError::from)?;
        Ok(posts.into_iter().map(Into::into).collect())
    }

    /// Posts a member-authored message into the Private Fleet channel from this machine -- the
    /// same action as typing into the web app's channel input.
    pub async fn channel_post(self: Arc<Self>, body: String) -> Result<ChannelPost, HiveError> {
        self.log("info", "posting to your Private Fleet channel").await;
        let cfg = nodeconfig::load().map_err(HiveError::from)?;
        let key = cfg
            .node_key
            .clone()
            .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
        let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
        let result = RUNTIME
            .spawn(async move { hub.channel_post(&body).await })
            .await
            .map_err(|e| HiveError::Failed(format!("channel_post task panicked: {e}")))?
            .map_err(HiveError::from)?;
        Ok(result.into())
    }
}
