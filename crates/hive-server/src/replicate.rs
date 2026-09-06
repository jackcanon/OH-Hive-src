//! Pull-based replication (ADR-007, replication factor 2). Every `EVERY` the server asks the hub
//! for a plan — artifacts below their replication factor that it doesn't hold and some online
//! server does — then fetches each from the holder's `/a/<hash>`, verifies the sha256, stores,
//! and announces. Idempotent and cheap when there is nothing to do.

use crate::store::Store;
use ohhive_core::hub::{ArtifactAnnounce, HubClient};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};

pub const EVERY: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
struct PlanItem {
    hash: String,
    bytes: u64,
    mime: Option<String>,
    kind: Option<String>,
    project_id: Option<uuid::Uuid>,
    card_id: Option<uuid::Uuid>,
    from: String,
    from_name: Option<String>,
}

pub async fn tick(hub: &HubClient, store: &Arc<Store>, http: &reqwest::Client) -> usize {
    let plan: Vec<PlanItem> = match hub.replication_plan(20).await {
        Ok(v) => serde_json::from_value(v).unwrap_or_default(),
        Err(e) => {
            tracing::debug!("replication_plan: {e}");
            return 0;
        }
    };
    let mut done = 0;
    for it in plan {
        if store.stat(&it.hash).ok().flatten().is_some() {
            // we already have it (e.g. announce failed last time) — just re-announce
            announce(hub, &it).await;
            continue;
        }
        let resp = match http.get(&it.from).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::warn!(hash = %it.hash, from = %it.from, status = %r.status(), "replica fetch rejected");
                continue;
            }
            Err(e) => {
                tracing::warn!(hash = %it.hash, from = %it.from, "replica fetch failed: {e}");
                continue;
            }
        };
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(hash = %it.hash, "replica body failed: {e}");
                continue;
            }
        };
        let got = hex::encode(Sha256::digest(&body));
        if got != it.hash {
            tracing::error!(hash = %it.hash, got = %got, from = %it.from, "replica hash mismatch — holder is corrupt or lying");
            continue;
        }
        let mime = it.mime.clone().unwrap_or_else(|| "application/octet-stream".into());
        if let Err(e) = store.put(&body, &mime) {
            tracing::warn!(hash = %it.hash, "store failed: {e}");
            continue;
        }
        announce(hub, &it).await;
        tracing::info!(hash = %&it.hash[..12], bytes = it.bytes, from = ?it.from_name, "replicated");
        done += 1;
    }
    done
}

async fn announce(hub: &HubClient, it: &PlanItem) {
    let a = ArtifactAnnounce {
        hash: it.hash.clone(),
        bytes: it.bytes,
        mime: it.mime.clone().unwrap_or_else(|| "application/octet-stream".into()),
        kind: it.kind.clone().unwrap_or_else(|| "output".into()),
        project_id: it.project_id,
        card_id: it.card_id,
        uploaded_by: None,
    };
    if let Err(e) = hub.artifact_announce(&a).await {
        tracing::warn!(hash = %it.hash, "announce failed: {e}");
    }
}
