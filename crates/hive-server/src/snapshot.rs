//! Read-all snapshot (ADR-013 §A.5). The coordinator pulls `snapshot_source` from the hub every
//! `EVERY` seconds; every other server pulls the coordinator's copy; all of them serve it at
//! `GET /snapshot/latest`. Members authenticate with `?token=<jwt>` (checked once via
//! `hive_is_member`, cached 5 min); servers authenticate with `Authorization: Bearer <node key>`.
//! Content-addressed: the ETag is the sha256 of the body, so browsers revalidate for free.

use axum::{
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
};
use ohhive_core::hub::{HubClient, MemberClient};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};

pub const EVERY: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct Snapshot {
    latest: Arc<RwLock<Option<Latest>>>,
    member: Arc<MemberClient>,
    /// member jwt → verified at
    members: Arc<Mutex<HashMap<String, Instant>>>,
}

#[derive(Clone)]
pub struct Latest {
    pub hash: String,
    pub body: String,
    pub at: Instant,
    pub source: &'static str, // "hub" (we are coordinator) or "coordinator" (relayed)
}

#[derive(Deserialize)]
pub struct SnapQuery {
    pub token: Option<String>,
}

impl Snapshot {
    pub fn new(member: MemberClient) -> Self {
        Self { latest: Arc::new(RwLock::new(None)), member: Arc::new(member), members: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub async fn age_secs(&self) -> Option<u64> {
        self.latest.read().await.as_ref().map(|l| l.at.elapsed().as_secs())
    }

    /// Called every tick by the heartbeat loop with the current coordinator state.
    pub async fn refresh(&self, hub: &HubClient, is_coordinator: bool, coordinator_url: Option<&str>, my_key: &str) {
        if is_coordinator {
            match hub.snapshot_source().await {
                Ok(v) => self.set(v.to_string(), "hub").await,
                Err(e) => tracing::warn!("snapshot_source failed: {e}"),
            }
        } else if let Some(url) = coordinator_url {
            let url = format!("{}/snapshot/latest", url.trim_end_matches('/'));
            match reqwest::Client::new().get(&url).header("Authorization", format!("Bearer {my_key}")).send().await {
                Ok(r) if r.status().is_success() => {
                    if let Ok(body) = r.text().await {
                        self.set(body, "coordinator").await;
                    }
                }
                Ok(r) => tracing::debug!(status = %r.status(), "coordinator snapshot fetch rejected"),
                Err(e) => tracing::debug!("coordinator snapshot fetch failed: {e}"),
            }
        }
    }

    async fn set(&self, body: String, source: &'static str) {
        let hash = hex::encode(Sha256::digest(body.as_bytes()));
        *self.latest.write().await = Some(Latest { hash, body, at: Instant::now(), source });
    }

    async fn member_ok(&self, jwt: &str) -> bool {
        {
            let m = self.members.lock().await;
            if let Some(at) = m.get(jwt) {
                if at.elapsed() < Duration::from_secs(300) {
                    return true;
                }
            }
        }
        match self.member.rpc(jwt, "hive_is_member", serde_json::json!({})).await {
            Ok(v) if v.as_bool() == Some(true) => {
                self.members.lock().await.insert(jwt.to_string(), Instant::now());
                true
            }
            _ => false,
        }
    }
}

impl Snapshot {
    /// Decide auth then render. `node_ok` is the caller's node-key verifier (main.rs owns the cache).
    pub async fn respond(&self, headers: &HeaderMap, q: &SnapQuery, node_ok: bool) -> axum::response::Response {
        let authed = if let Some(t) = q.token.as_deref() { self.member_ok(t).await } else { node_ok };
        if !authed {
            return (StatusCode::UNAUTHORIZED, "member token or node key required").into_response();
        }
        let Some(l) = self.latest.read().await.clone() else {
            return (StatusCode::SERVICE_UNAVAILABLE, "no snapshot yet").into_response();
        };
        if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(&format!("\"{}\"", l.hash)) {
            return StatusCode::NOT_MODIFIED.into_response();
        }
        (
            [
                (header::CONTENT_TYPE, "application/json".to_string()),
                (header::ETAG, format!("\"{}\"", l.hash)),
                (header::CACHE_CONTROL, "private, max-age=5".to_string()),
                (header::HeaderName::from_static("x-hive-snapshot-age"), l.at.elapsed().as_secs().to_string()),
                (header::HeaderName::from_static("x-hive-snapshot-source"), l.source.to_string()),
            ],
            l.body,
        )
            .into_response()
    }

    pub fn has_node_key(headers: &HeaderMap) -> Option<String> {
        headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ").map(|s| s.trim().to_string())
    }
}
