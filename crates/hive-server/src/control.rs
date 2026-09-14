//! Additive, explicitly configured regional control pilot. The authoritative direct path is
//! untouched. Regional sessions retain only scoped delegation; raw node keys are rejected.
use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
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
use tokio::sync::{oneshot, Mutex, RwLock};
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
    delegation: String,
    token: Token,
}
struct AbortTask<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for AbortTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Pending {
    value: Value,
    waiters: Vec<oneshot::Sender<bool>>,
}
#[derive(Clone)]
pub struct Control {
    db: Arc<RwLock<Option<Arc<tokio_postgres::Client>>>>,
    config: tokio_postgres::Config,
    tls: postgres_native_tls::MakeTlsConnector,
    token_ttl_seconds: u32,
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
        let mut tls_builder = native_tls::TlsConnector::builder();
        if let Some(path) = std::env::var_os("HIVE_CTL_DATABASE_CA") {
            let pem = std::fs::read(path).context("read configured pilot database CA")?;
            tls_builder.add_root_certificate(
                native_tls::Certificate::from_pem(&pem).context("invalid pilot database CA")?,
            );
        }
        let tls = postgres_native_tls::MakeTlsConnector::new(tls_builder.build()?);
        config.connect_timeout(Duration::from_secs(5));
        config.options("-c statement_timeout=8000 -c lock_timeout=3000");
        let token_ttl_seconds = std::env::var("HIVE_CTL_TOKEN_TTL_SECONDS")
            .ok()
            .map(|s| s.parse::<u32>())
            .transpose()
            .context("invalid pilot token TTL")?
            .unwrap_or(900);
        anyhow::ensure!(
            (30..=900).contains(&token_ttl_seconds),
            "pilot token TTL must be 30..900 seconds"
        );
        Ok(Self {
            db: Default::default(),
            config,
            tls,
            token_ttl_seconds,
            secret: Arc::new(rand::random()),
            sessions: Default::default(),
            pending: Default::default(),
        })
    }
    async fn database(&self) -> std::result::Result<Arc<tokio_postgres::Client>, StatusCode> {
        self.db
            .read()
            .await
            .as_ref()
            .filter(|db| !db.is_closed())
            .cloned()
            .ok_or(StatusCode::SERVICE_UNAVAILABLE)
    }
    // One supervisor owns reconnects. In-flight writes are never replayed.
    async fn supervise(&self) {
        let mut attempt = 0u32;
        loop {
            match tokio::time::timeout(
                Duration::from_secs(8),
                self.config.connect(self.tls.clone()),
            )
            .await
            {
                Ok(Ok((client, connection))) => {
                    let client = Arc::new(client);
                    let driver = AbortTask(tokio::spawn(connection));
                    // Validate the configured gateway, not just TCP connectivity.
                    let healthy = client
                        .query_one("select hive.ctl_pilot_ready()", &[])
                        .await
                        .is_ok();
                    if healthy {
                        *self.db.write().await = Some(client.clone());
                        tracing::info!("pilot database ready");
                        attempt = 0;
                        loop {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            if tokio::time::timeout(
                                Duration::from_secs(3),
                                client.simple_query("select 1"),
                            )
                            .await
                            .map(|r| r.is_err())
                            .unwrap_or(true)
                            {
                                break;
                            }
                        }
                    }
                    *self.db.write().await = None;
                    drop(driver);
                }
                _ => {
                    *self.db.write().await = None;
                }
            }
            let delay = reconnect_delay(attempt);
            attempt = attempt.saturating_add(1);
            tracing::warn!(
                delay_ms = delay.as_millis(),
                "pilot database unavailable; reconnect scheduled"
            );
            tokio::time::sleep(delay).await;
        }
    }
    pub fn router(&self) -> Router {
        Router::new()
            .route("/hive/ctl/1/ready", get(ready))
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
        let db = self.database().await?;
        db.query_one(
            "select hive.ctl_pilot_call($1,$2,$3,$4)",
            &[&key, &id, &method, &params],
        )
        .await
        .and_then(|r| {
            r.try_get::<_, Option<Value>>(0)
                .map(|v| v.unwrap_or(Value::Null))
        })
        .map_err(|e| {
            tracing::warn!(code=?e.code().map(|c|c.code()),method,"pilot operation rejected");
            if e.is_closed() || e.code().is_none() {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::CONFLICT
            }
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
            .call(
                &s.delegation,
                s.token.id,
                "token",
                json!({"id":id,"token_ttl_seconds":self.token_ttl_seconds}),
            )
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
        let accepted: Vec<Uuid> = match self.database().await {
            Ok(db) => match db
                .query_one("select hive.ctl_pilot_heartbeats($1)", &[&json!(values)])
                .await
            {
                Ok(r) => r
                    .try_get::<_, Value>(0)
                    .ok()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default(),
                Err(_) => vec![],
            },
            Err(_) => vec![],
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
        let supervisor = self.clone();
        let supervisor = AbortTask(tokio::spawn(async move { supervisor.supervise().await }));
        let cloned = self.clone();
        let batch = AbortTask(tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                cloned.flush().await;
            }
        }));
        let result = axum::serve(listener, self.router())
            .with_graceful_shutdown(stop)
            .await;
        drop(supervisor);
        drop(batch);
        *self.db.write().await = None;
        self.flush().await;
        result?;
        Ok(())
    }
}
fn reconnect_delay(attempt: u32) -> Duration {
    let cap = (250u64.saturating_mul(1u64 << attempt.min(7))).min(30_000);
    Duration::from_millis(cap / 2 + rand::random::<u64>() % (cap / 2 + 1))
}
async fn ready(State(c): State<Control>) -> StatusCode {
    match c.database().await {
        Ok(db) => match tokio::time::timeout(
            Duration::from_secs(3),
            db.query_one("select hive.ctl_pilot_ready()", &[]),
        )
        .await
        {
            Ok(Ok(_)) => StatusCode::OK,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        },
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
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
    if key.len() != 72 || !key.starts_with("hive_dg_") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let id = Uuid::new_v4();
    let result = c
        .call(
            key,
            id,
            "auth",
            json!({"id":id,"token_ttl_seconds":c.token_ttl_seconds}),
        )
        .await
        .map_err(|e| {
            if e == StatusCode::SERVICE_UNAVAILABLE {
                e
            } else {
                StatusCode::UNAUTHORIZED
            }
        })?;
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
            delegation: key.into(),
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
            pending.entry(token.node_id).or_insert_with(||Pending {value:json!({"node_id":token.node_id,"token_id":token.id,"delegation_hash":hex::encode(Sha256::digest(s.delegation.as_bytes())),"rtt_ms":req.params["prev_rtt_ms"]}),waiters:vec![]}).waiters.push(tx);
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
                | "recover_leases"
        ) {
            return Err(StatusCode::BAD_REQUEST);
        }
        if req.method == "claim_card" {
            let mut snapshot = c
                .call(&s.delegation, token.id, "snapshot", json!({}))
                .await?;
            loop {
                let Some(id) = choose(&snapshot) else {
                    break json!({"status":"nothing_to_do"});
                };
                let result = c
                    .call(&s.delegation, token.id, "claim_card", json!({"card_id":id}))
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
            c.call(&s.delegation, token.id, &req.method, req.params)
                .await?
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
    #[test]
    fn reconnect_backoff_is_bounded_and_jittered() {
        for n in [0, 1, 5, 7, 99, u32::MAX] {
            let cap = (250u64.saturating_mul(1u64 << n.min(7))).min(30_000);
            for _ in 0..20 {
                let d = reconnect_delay(n).as_millis() as u64;
                assert!(d >= cap / 2 && d <= cap);
            }
        }
    }
    #[tokio::test]
    async fn startup_binds_and_reports_unready_without_database() {
        let control = Control::connect("host=127.0.0.1 port=1 user=unused dbname=unused")
            .await
            .unwrap();
        assert_eq!(
            ready(State(control.clone())).await,
            StatusCode::SERVICE_UNAVAILABLE
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            ("Bearer hive_dg_".to_owned() + &"a".repeat(64))
                .parse()
                .unwrap(),
        );
        assert!(matches!(
            auth(State(control), headers).await,
            Err(StatusCode::SERVICE_UNAVAILABLE)
        ));
    }
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
