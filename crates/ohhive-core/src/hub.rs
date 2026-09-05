//! Hub client — how a node talks to the source of record (ADR-001) without
//! ever holding a Supabase user JWT (ADR-004). Auth is a *node key* minted by
//! the owning member (migration 0002); every call is a PostgREST RPC.
//!
//! Endpoints are the `public.hive_*` wrappers until schema `hive` is exposed
//! in the project's API settings, after which `Content-Profile: hive` and the
//! unprefixed names work too.

use crate::capability::Capabilities;
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
    pub fn new(base: impl Into<String>, anon_key: impl Into<String>, node_key: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            anon_key: anon_key.into(),
            node_key: node_key.into(),
            http: reqwest::Client::new(),
        }
    }

    async fn rpc<T: for<'de> Deserialize<'de>>(&self, name: &str, body: serde_json::Value) -> Result<T, HubError> {
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
        let text = resp.text().await.map_err(|e| HubError::Transport(e.to_string()))?;
        if !status.is_success() {
            if text.contains("invalid_or_revoked_node_key") {
                return Err(HubError::BadKey);
            }
            return Err(HubError::Rejected(format!("{status}: {text}")));
        }
        serde_json::from_str(&text).map_err(|e| HubError::Rejected(format!("bad response: {e}: {text}")))
    }

    pub async fn whoami(&self) -> Result<WhoAmI, HubError> {
        self.rpc("hive_node_whoami", serde_json::json!({ "raw_key": self.node_key })).await
    }

    /// Publish capabilities and become eligible for work. Returns the node row as JSON.
    pub async fn check_in(&self, caps: &Capabilities, region: Option<&str>) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "hive_node_checkin",
            serde_json::json!({ "raw_key": self.node_key, "p_capabilities": caps, "p_region": region }),
        )
        .await
    }

    pub async fn heartbeat(&self) -> Result<String, HubError> {
        self.rpc("hive_node_heartbeat", serde_json::json!({ "raw_key": self.node_key })).await
    }

    /// Returns the resulting presence: "checked_out" or "draining" (lease held).
    pub async fn check_out(&self) -> Result<String, HubError> {
        self.rpc("hive_node_checkout", serde_json::json!({ "raw_key": self.node_key })).await
    }
}
