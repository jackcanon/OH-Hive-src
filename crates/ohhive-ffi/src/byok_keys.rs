//! FFI wrapper for BYOK key/model management from the Swift app (2026-09-13, Jack: "I'd like the
//! picker in the swift app as well, in case people aren't always logging into the website. They
//! need to be able to operate independent of each other").
//!
//! Until this file, `hive.member_keys` (a member's own Anthropic/OpenAI/Nous key, plus which
//! model to use with it -- see `byok_keys` migration and `chat.rs`'s BYOK chat) could only be
//! read or written by the web app, since every RPC for it was gated on `auth.uid()` -- a real
//! Supabase member session the Swift/CLI desktop app never holds (it authenticates with a raw
//! node key only, same as `feedback.rs`/`chat.rs`). `HubClient::member_key_status`/`_set`/
//! `_remove`/`_set_model` go through the node-key-resolved twin of each web RPC
//! (`hive_node_member_key_*`, migration 20260913120000_node_key_byok_management.sql) so this
//! surface is genuinely independent of ever visiting the web app.
//!
//! Same statelessness and error-shape conventions as `chat.rs`: no local caching of key status
//! (`SettingsView`-equivalent Swift UI is expected to call `byokKeysStatus()` after every
//! set/remove, the same way `HiveStore.refresh()` re-fetches after any config change), and a raw
//! key is only ever passed in transit to the hub -- this FFI surface never stores or logs one.

use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::hub::{
    ByokModel as HubByokModel, HubClient, MemberKeyInfo as HubMemberKeyInfo,
    MemberKeysStatus as HubMemberKeysStatus,
};
use hive_core::nodeconfig;
use std::sync::Arc;

#[derive(uniffi::Record, Clone)]
pub struct ByokKeyInfo {
    pub last4: String,
    pub since: String,
    /// `None` means "use whatever this provider defaults to" -- never a blank string.
    pub preferred_model: Option<String>,
}

impl From<HubMemberKeyInfo> for ByokKeyInfo {
    fn from(k: HubMemberKeyInfo) -> Self {
        Self {
            last4: k.last4,
            since: k.since,
            preferred_model: k.preferred_model,
        }
    }
}

/// One field per known provider rather than a map -- see `hive_core::hub::MemberKeysStatus`'s doc
/// for why. `None` means no key is on file for that provider yet.
#[derive(uniffi::Record, Clone, Default)]
pub struct ByokKeysStatus {
    pub anthropic: Option<ByokKeyInfo>,
    pub openai: Option<ByokKeyInfo>,
    pub nous: Option<ByokKeyInfo>,
}

impl From<HubMemberKeysStatus> for ByokKeysStatus {
    fn from(s: HubMemberKeysStatus) -> Self {
        Self {
            anthropic: s.anthropic.map(Into::into),
            openai: s.openai.map(Into::into),
            nous: s.nous.map(Into::into),
        }
    }
}

/// One live model entry from a provider's own catalog (2026-09-15, see `HubClient::list_byok_models`'s
/// doc for the "not actually loading with models" report this answers). `label`, when present, is
/// a friendlier display name than `id` alone -- currently only Anthropic's models endpoint returns
/// one.
#[derive(uniffi::Record, Clone)]
pub struct ByokModelInfo {
    pub id: String,
    pub label: Option<String>,
}

impl From<HubByokModel> for ByokModelInfo {
    fn from(m: HubByokModel) -> Self {
        Self {
            id: m.id,
            label: m.label,
        }
    }
}

/// Loads this node's config and builds a `HubClient`, the same three lines every method below
/// (and `chat.rs`'s methods) needs -- pulled out once here since this file has four call sites
/// instead of chat.rs's two.
fn hub_client() -> Result<HubClient, HiveError> {
    let cfg = nodeconfig::load().map_err(HiveError::from)?;
    let key = cfg
        .node_key
        .clone()
        .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
    Ok(HubClient::new(&cfg.hub_url, &cfg.anon_key, key))
}

#[uniffi::export]
impl HiveNode {
    /// Every provider's current key status (last 4 chars, when added, preferred model if set).
    /// Never returns the key itself -- it isn't in the RPC's response to begin with.
    pub async fn byok_keys_status(self: Arc<Self>) -> Result<ByokKeysStatus, HiveError> {
        let hub = hub_client()?;
        let result = RUNTIME
            .spawn(async move { hub.member_key_status().await })
            .await
            .map_err(|e| HiveError::Failed(format!("byok_keys_status task panicked: {e}")))?
            .map_err(HiveError::from)?;
        Ok(result.into())
    }

    /// `provider` is "anthropic" | "openai" | "nous". Replaces any existing key for that provider
    /// (its preferred_model is reset -- same behavior as the web app's Settings page, see
    /// `hive.member_key_set_for`'s doc).
    pub async fn set_byok_key(
        self: Arc<Self>,
        provider: String,
        key: String,
    ) -> Result<(), HiveError> {
        self.log("info", format!("saving your {provider} key"))
            .await;
        let this = self.clone();
        let r = RUNTIME
            .spawn(async move {
                let hub = hub_client()?;
                hub.member_key_set(&provider, &key)
                    .await
                    .map_err(HiveError::from)
            })
            .await
            .map_err(|e| HiveError::Failed(format!("set_byok_key task panicked: {e}")))?;
        match &r {
            Ok(_) => this.log("ok", "key saved").await,
            Err(e) => this.log("error", format!("saving key failed: {e}")).await,
        }
        r
    }

    /// Returns `false` (not an error) if there was no key on file for `provider`.
    pub async fn remove_byok_key(self: Arc<Self>, provider: String) -> Result<bool, HiveError> {
        let hub = hub_client()?;
        let result = RUNTIME
            .spawn(async move { hub.member_key_remove(&provider).await })
            .await
            .map_err(|e| HiveError::Failed(format!("remove_byok_key task panicked: {e}")))?
            .map_err(HiveError::from)?;
        self.notify_changed().await;
        Ok(result)
    }

    /// `model` empty clears the override back to this provider's default. Fails with a friendly
    /// message (translated from `no_key_for_provider`) if `provider` has no key on file yet --
    /// the model field has nothing to attach to until a key exists.
    pub async fn set_byok_key_model(
        self: Arc<Self>,
        provider: String,
        model: String,
    ) -> Result<(), HiveError> {
        let hub = hub_client()?;
        RUNTIME
            .spawn(async move { hub.member_key_set_model(&provider, &model).await })
            .await
            .map_err(|e| HiveError::Failed(format!("set_byok_key_model task panicked: {e}")))?
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("no_key_for_provider") {
                    HiveError::Failed("add a key for this provider first".into())
                } else {
                    HiveError::from(e)
                }
            })
    }

    /// Live model catalog for `provider` ("anthropic" | "openai" | "nous"), fetched from that
    /// provider's own API using this node's owning member's stored key -- see
    /// `HubClient::list_byok_models`'s doc for the full Edge Function path (2026-09-15, "not
    /// actually loading with models"). Fails the same friendly way `set_byok_key_model` does when
    /// there's no key on file yet for `provider` -- the picker itself should only call this for a
    /// provider `byokKeysStatus()` already says is configured, but this is the source of truth if
    /// that state is ever stale.
    pub async fn list_byok_models(
        self: Arc<Self>,
        provider: String,
    ) -> Result<Vec<ByokModelInfo>, HiveError> {
        let hub = hub_client()?;
        let result = RUNTIME
            .spawn(async move { hub.list_byok_models(&provider).await })
            .await
            .map_err(|e| HiveError::Failed(format!("list_byok_models task panicked: {e}")))?
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("provider_key_not_configured") {
                    HiveError::Failed("add a key for this provider first".into())
                } else {
                    HiveError::from(e)
                }
            })?;
        Ok(result.into_iter().map(Into::into).collect())
    }
}
