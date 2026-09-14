//! Additive, explicitly configured regional control pilot. The authoritative direct path is
//! untouched. DB credentials and raw bootstrap keys never appear in responses or logs.
use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use hive_coordinator::{place, NodeView, Placement};
use hive_core::{
    capability::{Capabilities, Requirements},
    coordinator_hub::{ControlReply, ControlRequest},
    job::{Job, JobKind},
    node::{Presence, Region},
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

type ApiResult = std::result::Result<Json<ControlReply>, StatusCode>;
#[derive(Clone, Serialize, Deserialize)]
struct Token {
    id: Uuid,
    node_id: Uuid,
    server_id: Uuid,
    project_id: Uuid,
    lease_ids: Vec<Uuid>,
    expires_at: i64,
}
struct Session {
    key: String,
    token: Token,
}
struct Pending {
    value: Value,
    waiters: Vec<oneshot::Sender<bool>>,
}
#[derive(Clone)]
pub struct Control {
    db: Arc<tokio_postgres::Client>,
    secret: Arc<[u8; 32]>,
    sessions: Arc<Mutex<HashMap<Uuid, Arc<Mutex<Session>>>>>,
    pending: Arc<Mutex<HashMap<Uuid, Pending>>>,
}
impl Control {
    pub async fn connect(url: &str) -> Result<Self> {
        let config: tokio_postgres::Config = url
            .parse()
            .context("invalid pilot database configuration")?;
        // Supabase requires TLS; native TLS verifies the server certificate and hostname.
        let mut config = config;
        config.ssl_mode(tokio_postgres::config::SslMode::Require);
        let tls = postgres_native_tls::MakeTlsConnector::new(
            native_tls::TlsConnector::builder().build()?,
        );
        let (db, connection) = config
            .connect(tls)
            .await
            .map_err(|e| anyhow::anyhow!("pilot database connection failed: {e:?}"))?;
        tokio::spawn(async move {
            if connection.await.is_err() {
                tracing::error!("pilot database connection closed");
            }
        });
        Ok(Self {
            db: Arc::new(db),
            secret: Arc::new(rand::random()),
            sessions: Default::default(),
            pending: Default::default(),
        })
    }
    pub fn router(&self) -> Router {
        Router::new()
            .route("/hive/ctl/1/auth", post(auth))
            .route("/hive/ctl/1/rpc", post(rpc))
            .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
            .with_state(self.clone())
    }
    async fn call(
        &self,
        key: &str,
        id: Uuid,
        method: &str,
        params: Value,
    ) -> std::result::Result<Value, StatusCode> {
        self.db
            .query_one(
                "select hive.ctl_pilot_call($1,$2,$3,$4)",
                &[&key, &id, &method, &params],
            )
            .await
            .map(|r| r.get(0))
            .map_err(|e| {
                tracing::warn!(code=?e.code().map(|c|c.code()),method,"pilot operation rejected");
                StatusCode::CONFLICT
            })
    }
    fn sign(&self, t: &Token) -> String {
        sign_token(&self.secret, t)
    }
    fn verify(&self, s: &str) -> std::result::Result<Token, StatusCode> {
        verify_token(&self.secret, s)
    }
    async fn refresh(&self, s: &mut Session) -> std::result::Result<(), StatusCode> {
        let id = Uuid::new_v4();
        let result = self
            .call(&s.key, s.token.id, "token", json!({"id":id}))
            .await?;
        s.token.id = id;
        s.token.lease_ids = serde_json::from_value(result["lease_ids"].clone())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        s.token.expires_at = result["expires_at"]
            .as_i64()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(())
    }
    pub async fn flush(&self) {
        let entries = std::mem::take(&mut *self.pending.lock().await);
        if entries.is_empty() {
            return;
        }
        let values: Vec<_> = entries.values().map(|x| x.value.clone()).collect();
        let started = std::time::Instant::now();
        let accepted: Vec<Uuid> = match self
            .db
            .query_one("select hive.ctl_pilot_heartbeats($1)", &[&json!(values)])
            .await
        {
            Ok(r) => serde_json::from_value(r.get(0)).unwrap_or_default(),
            Err(_) => {
                tracing::warn!("pilot heartbeat batch failed");
                vec![]
            }
        };
        tracing::info!(
            nodes = entries.len(),
            accepted = accepted.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "pilot heartbeat batch"
        );
        for (id, p) in entries {
            for tx in p.waiters {
                let _ = tx.send(accepted.contains(&id));
            }
        }
    }
    pub async fn serve(
        self,
        listen: &str,
        stop: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(listen).await?;
        let cloned = self.clone();
        let batch = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                cloned.flush().await;
            }
        });
        let result = axum::serve(listener, self.router())
            .with_graceful_shutdown(stop)
            .await;
        batch.abort();
        self.flush().await;
        result?;
        Ok(())
    }
}
fn bearer(headers: &HeaderMap) -> std::result::Result<&str, StatusCode> {
    headers
        .get("authorization")
        .and_then(|x| x.to_str().ok())
        .and_then(|x| x.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)
}
async fn auth(State(c): State<Control>, headers: HeaderMap) -> ApiResult {
    let key = bearer(&headers)?;
    if key.len() != 56 || !key.starts_with("hive_nk_") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let id = Uuid::new_v4();
    let result = c
        .call(key, id, "auth", json!({"id":id}))
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let mut value = result.clone();
    value["id"] = json!(id);
    let token: Token =
        serde_json::from_value(value).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let signed = c.sign(&token);
    let mut sessions = c.sessions.lock().await;
    // One session per allowlisted worker. Re-authentication invalidates its previous bearer.
    sessions.insert(
        token.node_id,
        Arc::new(Mutex::new(Session {
            key: key.into(),
            token,
        })),
    );
    Ok(Json(ControlReply {
        result,
        token: signed,
    }))
}
fn choose(snapshot: &Value) -> Option<Uuid> {
    let n = &snapshot["node"];
    let caps: Capabilities = serde_json::from_value(n["capabilities"].clone()).ok()?;
    let node = NodeView {
        id: serde_json::from_value(n["id"].clone()).ok()?,
        region: Region(n["region"].as_str().unwrap_or("").into()),
        presence: serde_json::from_value(n["presence"].clone()).unwrap_or(Presence::CheckedOut),
        capabilities: caps,
        active_leases: snapshot["active_leases"].as_u64()? as usize,
    };
    for card in snapshot["cards"].as_array()? {
        let mut value = serde_json::to_value(Requirements {
            tools_level: hive_core::capability::ToolsLevel::InferenceOnly,
            ..Default::default()
        })
        .ok()?;
        for (k, v) in card["required_capabilities"].as_object()? {
            value[k] = v.clone();
        }
        let mut requirements: Requirements = serde_json::from_value(value).ok()?;
        requirements.modality = serde_json::from_value(card["modality"].clone()).ok();
        requirements.requires_internet = card["requires_internet"].as_bool().unwrap_or(false);
        let id = serde_json::from_value(card["id"].clone()).ok()?;
        let job = Job {
            id,
            kind: JobKind::AgentCard,
            project_id: serde_json::from_value(card["project_id"].clone()).ok()?,
            card_id: Some(id),
            parent: None,
            requirements,
            input: Value::Null,
            resume_from: None,
            created_at: chrono::Utc::now(),
        };
        if place(&job, std::slice::from_ref(&node), Some(&node.region)) == Placement::Node(node.id)
        {
            return Some(id);
        }
    }
    None
}
async fn rpc(
    State(c): State<Control>,
    headers: HeaderMap,
    Json(req): Json<ControlRequest>,
) -> ApiResult {
    let token = c.verify(bearer(&headers)?)?;
    let session = c
        .sessions
        .lock()
        .await
        .get(&token.node_id)
        .cloned()
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let mut s = session.lock().await;
    if s.token.id != token.id {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let started = std::time::Instant::now();
    let result = if req.method == "heartbeat" {
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = c.pending.lock().await;
            pending.entry(token.node_id).or_insert_with(||Pending {value:json!({"node_id":token.node_id,"token_id":token.id,"key_hash":hex::encode(Sha256::digest(s.key.as_bytes())),"rtt_ms":req.params["prev_rtt_ms"]}),waiters:vec![]}).waiters.push(tx);
        }
        if !tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or(false)
        {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        json!([
            chrono::Utc::now().to_rfc3339(),
            started.elapsed().as_millis() as u64
        ])
    } else {
        if !matches!(
            req.method.as_str(),
            "check_in"
                | "check_out"
                | "claim_card"
                | "checkpoint"
                | "complete_card"
                | "fail_card"
                | "release_card"
                | "spawn_child_card"
                | "wait_on_child"
                | "get_schedule"
                | "mcp_server_config"
                | "post_activity"
        ) {
            return Err(StatusCode::BAD_REQUEST);
        }
        if req.method == "claim_card" {
            let mut snapshot = c.call(&s.key, token.id, "snapshot", json!({})).await?;
            loop {
                let Some(id) = choose(&snapshot) else {
                    break json!({"status":"nothing_to_do"});
                };
                let result = c
                    .call(&s.key, token.id, "claim_card", json!({"card_id":id}))
                    .await?;
                if result["status"] != "nothing_to_do" {
                    break result;
                }
                // The database may reject a stale candidate or a funding/MCP/dependency gate.
                // Continue through this scoped snapshot instead of starving later cards.
                if let Some(cards) = snapshot["cards"].as_array_mut() {
                    cards.retain(|card| card["id"] != json!(id));
                }
            }
        } else {
            c.call(&s.key, token.id, &req.method, req.params).await?
        }
    };
    if matches!(
        req.method.as_str(),
        "claim_card" | "complete_card" | "release_card" | "fail_card" | "wait_on_child"
    ) || s.token.expires_at - chrono::Utc::now().timestamp() < 120
    {
        c.refresh(&mut s).await?;
    }
    tracing::info!(method=%req.method,elapsed_ms=started.elapsed().as_millis(),"pilot control request");
    Ok(Json(ControlReply {
        result,
        token: c.sign(&s.token),
    }))
}

fn sign_token(secret: &[u8; 32], t: &Token) -> String {
    let payload = hex::encode(serde_json::to_vec(t).expect("token serialize"));
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC key");
    mac.update(payload.as_bytes());
    format!("{}.{}", payload, hex::encode(mac.finalize().into_bytes()))
}
fn verify_token(secret: &[u8; 32], s: &str) -> std::result::Result<Token, StatusCode> {
    if s.len() > 8192 {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let (p, sig) = s.split_once('.').ok_or(StatusCode::UNAUTHORIZED)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC key");
    mac.update(p.as_bytes());
    mac.verify_slice(&hex::decode(sig).map_err(|_| StatusCode::UNAUTHORIZED)?)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let token: Token =
        serde_json::from_slice(&hex::decode(p).map_err(|_| StatusCode::UNAUTHORIZED)?)
            .map_err(|_| StatusCode::UNAUTHORIZED)?;
    if token.expires_at <= chrono::Utc::now().timestamp() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn token() -> Token {
        Token {
            id: Uuid::new_v4(),
            node_id: Uuid::new_v4(),
            server_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            lease_ids: vec![Uuid::new_v4()],
            expires_at: chrono::Utc::now().timestamp() + 900,
        }
    }
    #[test]
    fn tokens_bind_identity_project_server_and_leases() {
        let t = token();
        let signed = sign_token(&[1; 32], &t);
        let parsed = verify_token(&[1; 32], &signed).unwrap();
        assert_eq!(parsed.node_id, t.node_id);
        assert_eq!(parsed.lease_ids, t.lease_ids);
        assert_eq!(parsed.project_id, t.project_id);
        assert_eq!(parsed.server_id, t.server_id);
        assert!(verify_token(&[2; 32], &signed).is_err());
        let mut changed = t.clone();
        changed.node_id = Uuid::new_v4();
        let forged = format!(
            "{}.{}",
            hex::encode(serde_json::to_vec(&changed).unwrap()),
            signed.split_once('.').unwrap().1
        );
        assert!(verify_token(&[1; 32], &forged).is_err());
    }
    #[test]
    fn tokens_reject_expired_malformed_and_oversized_input() {
        let mut t = token();
        t.expires_at = chrono::Utc::now().timestamp();
        assert!(verify_token(&[1; 32], &sign_token(&[1; 32], &t)).is_err());
        for value in ["".into(), "aa.bb".into(), "x".repeat(8193)] {
            assert!(verify_token(&[1; 32], &value).is_err());
        }
    }
    fn snapshot() -> Value {
        json!({"active_leases":0,"node":{"id":Uuid::new_v4(),"region":"us-east","presence":"checked_in","capabilities":{
 "hardware":{"cpu_model":"fixture","cpu_cores":4,"ram_bytes":17179869184u64,"gpu_vendor":"none","disk_free_bytes":17179869184u64},
 "modalities":["text"],"models":[{"id":"test-model","modality":"text","backend":"llama_cpp"}],"allow_internet":false,"tools_level":"inference_only"}},
 "cards":[{"id":Uuid::new_v4(),"project_id":Uuid::new_v4(),"modality":"text","requires_internet":false,"required_capabilities":{}}]})
    }
    #[test]
    fn scheduler_preserves_inference_only_default_and_model_memory_gates() {
        let mut s = snapshot();
        assert!(choose(&s).is_some());
        s["cards"][0]["required_capabilities"] = json!({"model_id":"missing"});
        assert!(choose(&s).is_none());
        s["cards"][0]["required_capabilities"] = json!({"min_ram_bytes":999999999999u64});
        assert!(choose(&s).is_none());
        s["cards"][0]["required_capabilities"] = json!({});
        s["cards"][0]["requires_internet"] = json!(true);
        assert!(choose(&s).is_none());
    }
    #[test]
    fn scheduler_refuses_busy_checked_out_or_wrong_modality_workers() {
        let mut s = snapshot();
        s["active_leases"] = json!(1);
        assert!(choose(&s).is_none());
        s["active_leases"] = json!(0);
        s["node"]["presence"] = json!("checked_out");
        assert!(choose(&s).is_none());
        s["node"]["presence"] = json!("checked_in");
        s["cards"][0]["modality"] = json!("image");
        assert!(choose(&s).is_none());
    }
}
