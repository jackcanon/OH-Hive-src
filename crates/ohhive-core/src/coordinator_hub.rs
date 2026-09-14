//! Opt-in regional control transport. Never selected as the default and never retries writes
//! through Supabase. Account/artifact adapters remain explicitly community-backed.
use crate::{capability::Capabilities, hub::*};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

pub struct CoordinatorHub {
    base: String,
    http: reqwest::Client,
    token: Mutex<String>,
    community: Option<HubClient>,
}
impl CoordinatorHub {
    pub async fn connect(
        base: &str,
        node_key: &str,
        community: Option<HubClient>,
    ) -> Result<Self, HubError> {
        let url = reqwest::Url::parse(base)
            .map_err(|_| HubError::Rejected("invalid coordinator URL".into()))?;
        if url.scheme() != "https"
            && !(url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost")))
        {
            return Err(HubError::Rejected(
                "coordinator requires HTTPS (loopback excepted)".into(),
            ));
        }
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(HubError::Rejected("invalid coordinator URL".into()));
        }
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|_| HubError::Transport("client setup failed".into()))?;
        let response = http
            .post(format!("{}/hive/ctl/1/auth", base.trim_end_matches('/')))
            .bearer_auth(node_key)
            .send()
            .await
            .map_err(|_| HubError::Transport("coordinator unreachable".into()))?;
        if !response.status().is_success() {
            return Err(HubError::BadKey);
        }
        let reply: ControlReply = response
            .json()
            .await
            .map_err(|_| HubError::Rejected("invalid coordinator reply".into()))?;
        Ok(Self {
            base: base.trim_end_matches('/').into(),
            http,
            token: Mutex::new(reply.token),
            community,
        })
    }
    async fn rpc<T: for<'a> Deserialize<'a>>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, HubError> {
        // Serialize calls so a lease-changing response cannot race a heartbeat token renewal.
        let mut token = self.token.lock().await;
        let response = self
            .http
            .post(format!("{}/hive/ctl/1/rpc", self.base))
            .bearer_auth(&*token)
            .json(&ControlRequest {
                method: method.into(),
                params,
            })
            .send()
            .await
            .map_err(|_| {
                HubError::Transport("coordinator unreachable; operation was not replayed".into())
            })?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(HubError::BadKey);
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
        serde_json::from_value(reply.result)
            .map_err(|_| HubError::Rejected("invalid coordinator result".into()))
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
        routing::post,
        Json, Router,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[tokio::test]
    async fn bootstrap_key_is_exchanged_once_and_rotated_token_is_used() {
        #[derive(Clone)]
        struct Seen(Arc<AtomicUsize>);
        async fn auth(State(s): State<Seen>, h: HeaderMap) -> Json<ControlReply> {
            assert_eq!(h["authorization"], "Bearer synthetic-bootstrap-key");
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
            .route("/hive/ctl/1/auth", post(auth))
            .route("/hive/ctl/1/rpc", post(rpc))
            .with_state(seen.clone());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let hub = CoordinatorHub::connect(&origin, "synthetic-bootstrap-key", None)
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
}
