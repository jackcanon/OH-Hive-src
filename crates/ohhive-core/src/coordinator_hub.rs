//! Opt-in regional control transport. Never selected as the default and never retries writes
//! through Supabase. Account/artifact adapters remain explicitly community-backed.
use crate::{capability::Capabilities, hub::*};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct ControlRequest {
    pub method: String,
    pub params: Value,
}
#[derive(Serialize, Deserialize)]
pub struct ControlReply {
    pub result: Value,
    pub token: String,
}

/// Runs on the worker. Implementations may refresh a delegation from a trusted authority;
/// never send a node key to a regional server or follow a server-provided redirect.
#[async_trait::async_trait]
pub trait DelegationSource: Send + Sync {
    async fn credential(&self) -> Result<String, HubError>;
}
struct FixedDelegation(String);
#[async_trait::async_trait]
impl DelegationSource for FixedDelegation {
    async fn credential(&self) -> Result<String, HubError> {
        Ok(self.0.clone())
    }
}
/// Explicit trusted authority, separate from the regional endpoint. The raw node key stays here.
pub struct AuthorityDelegation {
    endpoint: String,
    anon_key: String,
    node_key: String,
    server_id: Uuid,
    project_id: Uuid,
}
impl AuthorityDelegation {
    pub fn new(
        authority: &str,
        anon_key: String,
        node_key: String,
        server_id: Uuid,
        project_id: Uuid,
    ) -> Result<Self, HubError> {
        validate_origin(authority)?;
        Ok(Self {
            endpoint: format!(
                "{}/rest/v1/rpc/hive_control_delegate",
                authority.trim_end_matches('/')
            ),
            anon_key,
            node_key,
            server_id,
            project_id,
        })
    }
}
#[async_trait::async_trait]
impl DelegationSource for AuthorityDelegation {
    async fn credential(&self) -> Result<String, HubError> {
        let response = http_client()?.post(&self.endpoint).header("apikey", &self.anon_key)
            .bearer_auth(&self.anon_key).json(&json!({"raw_key":self.node_key,"p_server":self.server_id,"p_project":self.project_id}))
            .send().await.map_err(|_| HubError::Transport("delegation authority unavailable".into()))?;
        if !response.status().is_success() {
            return Err(HubError::Rejected("delegation issuance rejected".into()));
        }
        let v: Value = response
            .json()
            .await
            .map_err(|_| HubError::Rejected("invalid delegation reply".into()))?;
        v["delegation"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| HubError::Rejected("missing delegation".into()))
    }
}
fn validate_origin(base: &str) -> Result<(), HubError> {
    let u = reqwest::Url::parse(base)
        .map_err(|_| HubError::Rejected("invalid coordinator URL".into()))?;
    if !(u.scheme() == "https"
        || u.scheme() == "http"
            && matches!(u.host_str(), Some("127.0.0.1" | "[::1]" | "localhost")))
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.path() != "/"
    {
        return Err(HubError::Rejected(
            "trusted HTTPS origin required (loopback excepted)".into(),
        ));
    }
    Ok(())
}
fn http_client() -> Result<reqwest::Client, HubError> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| HubError::Transport("client setup failed".into()))
}

pub struct CoordinatorHub {
    base: String,
    http: reqwest::Client,
    token: Mutex<String>,
    community: Option<HubClient>,
    source: Arc<dyn DelegationSource>,
}
impl CoordinatorHub {
    pub async fn connect(
        base: &str,
        delegation: &str,
        community: Option<HubClient>,
    ) -> Result<Self, HubError> {
        Self::connect_with_source(
            base,
            Arc::new(FixedDelegation(delegation.into())),
            community,
        )
        .await
    }
    pub async fn connect_with_source(
        base: &str,
        source: Arc<dyn DelegationSource>,
        community: Option<HubClient>,
    ) -> Result<Self, HubError> {
        validate_origin(base)?;
        let hub = Self {
            base: base.trim_end_matches('/').into(),
            http: http_client()?,
            token: Mutex::new(String::new()),
            community,
            source,
        };
        *hub.token.lock().await = hub.authenticate().await?;
        Ok(hub)
    }
    /// Re-discover readiness only at the configured trusted origin. No automatic origin switch.
    async fn wait_ready(&self) -> Result<(), HubError> {
        for attempt in 0..5 {
            if self
                .http
                .get(format!("{}/hive/ctl/1/ready", self.base))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250 * (1 << attempt))).await;
        }
        Err(HubError::Transport(
            "coordinator not ready; retry later".into(),
        ))
    }
    async fn authenticate(&self) -> Result<String, HubError> {
        self.wait_ready().await?;
        let credential = self.source.credential().await?;
        if credential.starts_with("hive_nk_") {
            return Err(HubError::Rejected(
                "regional authentication requires a scoped delegation".into(),
            ));
        }
        let response = self
            .http
            .post(format!("{}/hive/ctl/1/auth", self.base))
            .bearer_auth(credential)
            .send()
            .await
            .map_err(|_| HubError::Transport("coordinator unavailable".into()))?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(HubError::BadKey);
        }
        if !response.status().is_success() {
            return Err(HubError::Transport(
                "coordinator authentication unavailable".into(),
            ));
        }
        let reply: ControlReply = response
            .json()
            .await
            .map_err(|_| HubError::Rejected("invalid authentication reply".into()))?;
        Ok(reply.token)
    }
    /// Reconciles authoritative, unexpired leases/checkpoints. Never claims, replays a completion,
    /// or extends an expired lease. The caller decides whether to resume its local worker state.
    pub async fn recover_leases(&self) -> Result<Vec<Value>, HubError> {
        self.rpc("recover_leases", json!({})).await
    }
    async fn rpc<T: for<'a> Deserialize<'a>>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, HubError> {
        // Serialize calls so a lease-changing response cannot race a heartbeat token renewal.
        let mut token = self.token.lock().await;
        let retry_safe = matches!(method, "heartbeat" | "get_schedule" | "recover_leases");
        let mut reauthenticated = false;
        for attempt in 0..4 {
            let response = self
                .http
                .post(format!("{}/hive/ctl/1/rpc", self.base))
                .bearer_auth(&*token)
                .json(&ControlRequest {
                    method: method.into(),
                    params: params.clone(),
                })
                .send()
                .await;
            let response = match response {
                Ok(r) => r,
                Err(_) if retry_safe && attempt < 3 => {
                    self.wait_ready().await?;
                    continue;
                }
                Err(_) => {
                    return Err(HubError::Transport(
                        "coordinator unreachable; operation was not replayed".into(),
                    ))
                }
            };
            if response.status() == reqwest::StatusCode::UNAUTHORIZED && !reauthenticated {
                // 401 is emitted before dispatch. Ambiguous writes (timeouts/5xx) never replay.
                *token = self.authenticate().await?;
                reauthenticated = true;
                continue;
            }
            if response.status().is_server_error() && retry_safe && attempt < 3 {
                self.wait_ready().await?;
                tokio::time::sleep(Duration::from_millis(250 * (1 << attempt))).await;
                continue;
            }
            if !response.status().is_success() {
                return Err(HubError::Rejected(format!(
                    "coordinator rejected operation ({})",
                    response.status()
                )));
            }
            let reply: ControlReply = response
                .json()
                .await
                .map_err(|_| HubError::Rejected("invalid coordinator reply".into()))?;
            *token = reply.token;
            return serde_json::from_value(reply.result)
                .map_err(|_| HubError::Rejected("invalid coordinator result".into()));
        }
        Err(HubError::BadKey)
    }
}
#[async_trait::async_trait]
impl Hub for CoordinatorHub {
    fn community_client(&self) -> Option<&HubClient> {
        self.community.as_ref()
    }
    async fn claim_card(&self) -> Result<Claim, HubError> {
        self.rpc("claim_card", json!({})).await
    }
    async fn complete_card(
        &self,
        card_id: Uuid,
        content: &str,
        model_id: Option<&str>,
        usage: crate::ledger::Usage,
    ) -> Result<Completion, HubError> {
        self.rpc(
            "complete_card",
            json!({"card_id": card_id, "content": content, "model_id": model_id, "usage": usage}),
        )
        .await
    }
    async fn checkpoint(
        &self,
        card_id: Uuid,
        step: u32,
        state: &serde_json::Value,
        usage: crate::ledger::Usage,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "checkpoint",
            json!({"card_id": card_id, "step": step, "state": state, "usage": usage}),
        )
        .await
    }
    async fn fail_card(&self, card_id: Uuid, reason: &str) -> Result<serde_json::Value, HubError> {
        self.rpc("fail_card", json!({"card_id": card_id, "reason": reason}))
            .await
    }
    async fn release_card(
        &self,
        card_id: Uuid,
        reason: &str,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "release_card",
            json!({"card_id": card_id, "reason": reason}),
        )
        .await
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
        self.rpc("spawn_child_card", json!({"parent_card_id": parent_card_id, "key": key, "title": title, "modality": modality, "inputs": inputs, "acceptance": acceptance, "required_capabilities": required_capabilities})).await
    }
    async fn wait_on_child(
        &self,
        card_id: Uuid,
        child_card_id: Uuid,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc(
            "wait_on_child",
            json!({"card_id": card_id, "child_card_id": child_card_id}),
        )
        .await
    }
    async fn mcp_server_config(&self, server_id: Uuid) -> Result<McpServerConfig, HubError> {
        self.rpc("mcp_server_config", json!({"server_id": server_id}))
            .await
    }
    async fn check_in(
        &self,
        caps: &Capabilities,
        region: Option<&str>,
    ) -> Result<serde_json::Value, HubError> {
        self.rpc("check_in", json!({"caps": caps, "region": region}))
            .await
    }
    async fn heartbeat(&self, prev_rtt_ms: Option<u64>) -> Result<(String, u64), HubError> {
        let started = std::time::Instant::now();
        let (timestamp, _): (String, u64) = self
            .rpc("heartbeat", json!({"prev_rtt_ms": prev_rtt_ms}))
            .await?;
        Ok((timestamp, started.elapsed().as_millis() as u64))
    }
    async fn check_out(&self) -> Result<String, HubError> {
        self.rpc("check_out", json!({})).await
    }
    async fn get_schedule(&self) -> Result<Option<serde_json::Value>, HubError> {
        self.rpc("get_schedule", json!({})).await
    }
    async fn post_activity(
        &self,
        event_type: &str,
        body: &str,
        payload: serde_json::Value,
    ) -> Result<(), HubError> {
        self.rpc(
            "post_activity",
            json!({"event_type": event_type, "body": body, "payload": payload}),
        )
        .await
    }
}

#[cfg(all(test, feature = "local-hub"))]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::{get, post},
        Json, Router,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[tokio::test]
    async fn delegation_is_exchanged_once_and_rotated_token_is_used() {
        #[derive(Clone)]
        struct Seen(Arc<AtomicUsize>);
        async fn auth(State(s): State<Seen>, h: HeaderMap) -> Json<ControlReply> {
            assert_eq!(h["authorization"], "Bearer synthetic-delegation");
            s.0.fetch_add(1, Ordering::SeqCst);
            Json(ControlReply {
                result: Value::Null,
                token: "first-token".into(),
            })
        }
        async fn rpc(
            State(s): State<Seen>,
            h: HeaderMap,
            Json(r): Json<ControlRequest>,
        ) -> Json<ControlReply> {
            let n = s.0.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                h["authorization"],
                if n == 1 {
                    "Bearer first-token"
                } else {
                    "Bearer renewed-token"
                }
            );
            assert_eq!(r.method, "get_schedule");
            Json(ControlReply {
                result: Value::Null,
                token: "renewed-token".into(),
            })
        }
        let seen = Seen(Arc::new(AtomicUsize::new(0)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/hive/ctl/1/ready", get(|| async { StatusCode::OK }))
            .route("/hive/ctl/1/auth", post(auth))
            .route("/hive/ctl/1/rpc", post(rpc))
            .with_state(seen.clone());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let hub = CoordinatorHub::connect(&origin, "synthetic-delegation", None)
            .await
            .unwrap();
        assert_eq!(hub.get_schedule().await.unwrap(), None);
        assert_eq!(hub.get_schedule().await.unwrap(), None);
        assert_eq!(seen.0.load(Ordering::SeqCst), 3);
        task.abort();
    }
    #[tokio::test]
    async fn rejected_write_is_not_replayed_or_sent_to_community() {
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = seen.clone();
        let router = Router::new()
            .route("/hive/ctl/1/ready", get(|| async { StatusCode::OK }))
            .route(
                "/hive/ctl/1/auth",
                post(|| async {
                    Json(ControlReply {
                        result: Value::Null,
                        token: "token".into(),
                    })
                }),
            )
            .route(
                "/hive/ctl/1/rpc",
                post(move || {
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let hub = CoordinatorHub::connect(
            &origin,
            "synthetic",
            Some(HubClient::new("http://127.0.0.1:1", "unused", "unused")),
        )
        .await
        .unwrap();
        assert!(hub
            .complete_card(Uuid::new_v4(), "test", None, Default::default())
            .await
            .is_err());
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        task.abort();
    }
    #[tokio::test]
    async fn non_loopback_plaintext_origin_is_rejected_before_authentication() {
        assert!(
            CoordinatorHub::connect("http://192.168.1.1", "secret", None)
                .await
                .is_err()
        );
        assert!(
            CoordinatorHub::connect("https://name:password@example.org", "secret", None)
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn restart_reauthenticates_before_retry_and_recovers_checkpoint() {
        #[derive(Clone)]
        struct Seen {
            auth: Arc<AtomicUsize>,
            writes: Arc<AtomicUsize>,
        }
        let seen = Seen {
            auth: Arc::new(AtomicUsize::new(0)),
            writes: Arc::new(AtomicUsize::new(0)),
        };
        async fn auth(State(s): State<Seen>) -> Json<ControlReply> {
            let n = s.auth.fetch_add(1, Ordering::SeqCst);
            Json(ControlReply {
                result: Value::Null,
                token: format!("session-{n}"),
            })
        }
        async fn rpc(
            State(s): State<Seen>,
            h: HeaderMap,
            Json(r): Json<ControlRequest>,
        ) -> Result<Json<ControlReply>, StatusCode> {
            if h["authorization"] == "Bearer session-0" {
                return Err(StatusCode::UNAUTHORIZED);
            }
            let result = if r.method == "recover_leases" {
                json!([{"card_id":"retained-card","checkpoint":{"step":3}}])
            } else {
                s.writes.fetch_add(1, Ordering::SeqCst);
                Value::Null
            };
            Ok(Json(ControlReply {
                result,
                token: "session-1".into(),
            }))
        }
        let router = Router::new()
            .route("/hive/ctl/1/ready", get(|| async { StatusCode::OK }))
            .route("/hive/ctl/1/auth", post(auth))
            .route("/hive/ctl/1/rpc", post(rpc))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let hub = CoordinatorHub::connect(&origin, "scoped-test-delegation", None)
            .await
            .unwrap();
        hub.post_activity("test", "test", Value::Null)
            .await
            .unwrap();
        assert_eq!(seen.auth.load(Ordering::SeqCst), 2);
        assert_eq!(seen.writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            hub.recover_leases().await.unwrap()[0]["checkpoint"]["step"],
            3
        );
        task.abort();
    }
    #[tokio::test]
    async fn heartbeat_recovers_transient_database_failure_without_replaying_writes() {
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = seen.clone();
        let router = Router::new()
            .route("/hive/ctl/1/ready", get(|| async { StatusCode::OK }))
            .route(
                "/hive/ctl/1/auth",
                post(|| async {
                    Json(ControlReply {
                        result: Value::Null,
                        token: "token".into(),
                    })
                }),
            )
            .route(
                "/hive/ctl/1/rpc",
                post(move || {
                    let c = counter.clone();
                    async move {
                        if c.fetch_add(1, Ordering::SeqCst) == 0 {
                            Err(StatusCode::SERVICE_UNAVAILABLE)
                        } else {
                            Ok(Json(ControlReply {
                                result: json!(["now", 0]),
                                token: "token".into(),
                            }))
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let hub = CoordinatorHub::connect(&origin, "scoped-test-delegation", None)
            .await
            .unwrap();
        hub.heartbeat(None).await.unwrap();
        assert_eq!(seen.load(Ordering::SeqCst), 2);
        task.abort();
    }
    #[tokio::test]
    async fn regional_endpoint_never_receives_raw_worker_key() {
        let seen = Arc::new(AtomicUsize::new(0));
        let c = seen.clone();
        let router = Router::new()
            .route("/hive/ctl/1/ready", get(|| async { StatusCode::OK }))
            .route(
                "/hive/ctl/1/auth",
                post(move || {
                    let c = c.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        StatusCode::OK
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        assert!(CoordinatorHub::connect(&origin, "hive_nk_secret", None)
            .await
            .is_err());
        assert_eq!(seen.load(Ordering::SeqCst), 0);
        task.abort();
    }
}
