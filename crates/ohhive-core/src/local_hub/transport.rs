use super::*;
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use std::{net::IpAddr, time::Duration};

#[derive(Deserialize)]
struct Request {
    session: Uuid,
    method: String,
    params: Value,
}
#[derive(Serialize, Deserialize)]
struct PairRequest {
    code: String,
    name: String,
}
fn argument<T: for<'a> Deserialize<'a>>(v: &Value, k: &str) -> Result<T> {
    serde_json::from_value(v.get(k).cloned().unwrap_or(Value::Null))
        .map_err(|_| rejected("invalid local request argument"))
}
fn wire<T: Serialize>(v: T) -> Result<Value> {
    serde_json::to_value(v).map_err(|_| rejected("invalid local response"))
}
fn response(result: Result<Value>) -> (StatusCode, Json<Value>) {
    match result {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(HubError::BadKey) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"invalid or revoked local node key"})),
        ),
        Err(HubError::Rejected(message)) => (StatusCode::CONFLICT, Json(json!({"error":message}))),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"local hub unavailable"})),
        ),
    }
}
async fn rpc(
    State(store): State<LocalHubStore>,
    headers: HeaderMap,
    Json(req): Json<Request>,
) -> (StatusCode, Json<Value>) {
    let key = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    let Some(key) = key else {
        return response(Err(HubError::BadKey));
    };
    let key = key.to_owned();
    // Keep SQLite waits off the HTTP runtime's executor threads.
    let result = tokio::task::spawn_blocking(move || {
        let hub = store.connect_session(&key, req.session)?;
        futures::executor::block_on(dispatch(&hub, &req.method, &req.params))
    })
    .await
    .unwrap_or_else(|_| Err(rejected("local request failed")));
    response(result)
}
async fn pair(
    State(store): State<LocalHubStore>,
    Json(req): Json<PairRequest>,
) -> (StatusCode, Json<Value>) {
    let result =
        tokio::task::spawn_blocking(move || wire(store.redeem_pairing(&req.code, &req.name)?))
            .await
            .unwrap_or_else(|_| Err(rejected("pairing failed")));
    response(result)
}
/// No CORS, cookies, anonymous reads, administration endpoints, or request logging.
/// Call serve() to enforce local bind restrictions; router() supports an owner-managed TLS proxy.
pub fn router(store: LocalHubStore) -> Router {
    Router::new()
        .route("/local/v1/rpc", post(rpc))
        .route("/local/v1/pair", post(pair))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(store)
}
fn local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local(),
        IpAddr::V6(v) => v.is_loopback() || v.is_unique_local() || v.is_unicast_link_local(),
    }
}
/// Defaults belong to the caller; use 127.0.0.1:8787, or an explicit private LAN address.
/// Wildcard/public binds are refused. TLS may be terminated by the existing private tunnel setup.
pub async fn serve(
    store: LocalHubStore,
    listener: tokio::net::TcpListener,
    stop: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let addr = listener
        .local_addr()
        .map_err(|_| rejected("invalid listener"))?;
    if !local_ip(addr.ip()) {
        return Err(rejected(
            "local hub must bind a loopback or private LAN address",
        ));
    }
    axum::serve(listener, router(store))
        .with_graceful_shutdown(stop)
        .await
        .map_err(|_| rejected("local server stopped unexpectedly"))
}
async fn dispatch(h: &LocalHub, m: &str, p: &Value) -> Result<Value> {
    match m {
        "claim_card" => wire(h.claim_card().await?),
        "complete_card" => wire(
            h.complete_card(
                argument(p, "card_id")?,
                &argument::<String>(p, "content")?,
                argument::<Option<String>>(p, "model_id")?.as_deref(),
                argument(p, "usage")?,
            )
            .await?,
        ),
        "checkpoint" => wire(
            h.checkpoint(
                argument(p, "card_id")?,
                argument(p, "step")?,
                &argument::<Value>(p, "state")?,
                argument(p, "usage")?,
            )
            .await?,
        ),
        "fail_card" => wire(
            h.fail_card(argument(p, "card_id")?, &argument::<String>(p, "reason")?)
                .await?,
        ),
        "release_card" => wire(
            h.release_card(argument(p, "card_id")?, &argument::<String>(p, "reason")?)
                .await?,
        ),
        "spawn_child_card" => wire(
            h.spawn_child_card(
                argument(p, "parent_card_id")?,
                &argument::<String>(p, "key")?,
                &argument::<String>(p, "title")?,
                &argument::<String>(p, "modality")?,
                &argument::<String>(p, "inputs")?,
                &argument::<String>(p, "acceptance")?,
                argument(p, "required_capabilities")?,
            )
            .await?,
        ),
        "wait_on_child" => wire(
            h.wait_on_child(argument(p, "card_id")?, argument(p, "child_card_id")?)
                .await?,
        ),
        "mcp_server_config" => wire(h.mcp_server_config(argument(p, "server_id")?).await?),
        "check_in" => wire(
            h.check_in(
                &argument::<Capabilities>(p, "caps")?,
                argument::<Option<String>>(p, "region")?.as_deref(),
            )
            .await?,
        ),
        "heartbeat" => wire(h.heartbeat(argument(p, "prev_rtt_ms")?).await?),
        "check_out" => wire(h.check_out().await?),
        "get_schedule" => wire(h.get_schedule().await?),
        "post_activity" => wire(
            h.post_activity(
                &argument::<String>(p, "event_type")?,
                &argument::<String>(p, "body")?,
                argument(p, "payload")?,
            )
            .await?,
        ),
        _ => Err(rejected("unsupported local method")),
    }
}

/// Transport for the same Hub contract. Never holds a Supabase credential/client.
#[derive(Clone)]
pub struct RemoteLocalHub {
    base: String,
    key: String,
    session: Uuid,
    http: reqwest::Client,
}
fn client(base: &str) -> Result<(String, reqwest::Client)> {
    let url = reqwest::Url::parse(base).map_err(|_| rejected("invalid local hub URL"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(rejected(
            "local hub URL must be an origin without credentials",
        ));
    }
    let is_lan = url
        .host_str()
        .and_then(|s| s.trim_matches(['[', ']']).parse::<IpAddr>().ok())
        .map(local_ip)
        .unwrap_or(url.host_str() == Some("localhost"));
    if url.scheme() != "https" && !(url.scheme() == "http" && is_lan) {
        return Err(rejected(
            "use HTTPS for remote hubs; plain HTTP is restricted to numeric LAN/loopback addresses",
        ));
    }
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| rejected("cannot create local client"))?;
    Ok((url.as_str().trim_end_matches('/').into(), http))
}
impl RemoteLocalHub {
    pub fn new(base: &str, key: String) -> Result<Self> {
        let (base, http) = client(base)?;
        Ok(Self {
            base,
            key,
            session: Uuid::new_v4(),
            http,
        })
    }
    pub async fn pair(base: &str, code: &str, name: &str) -> Result<NodeCredentials> {
        let (base, http) = client(base)?;
        let r = http
            .post(format!("{base}/local/v1/pair"))
            .json(&PairRequest {
                code: code.into(),
                name: name.into(),
            })
            .send()
            .await
            .map_err(|_| rejected("local pairing connection failed"))?;
        if !r.status().is_success() {
            return Err(rejected("pairing code invalid, expired, or exhausted"));
        }
        r.json()
            .await
            .map_err(|_| rejected("invalid pairing response"))
    }
    async fn rpc<T: for<'a> Deserialize<'a>>(&self, method: &str, params: Value) -> Result<T> {
        let r = self
            .http
            .post(format!("{}/local/v1/rpc", self.base))
            .bearer_auth(&self.key)
            .json(&json!({"session":self.session,"method":method,"params":params}))
            .send()
            .await
            .map_err(|_| HubError::Transport("local hub unreachable".into()))?;
        if r.status() == StatusCode::UNAUTHORIZED {
            return Err(HubError::BadKey);
        }
        if !r.status().is_success() {
            return Err(rejected("local operation rejected; inspect hub state"));
        }
        r.json()
            .await
            .map_err(|_| rejected("invalid local hub response"))
    }
}

#[async_trait::async_trait]
impl Hub for RemoteLocalHub {
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
        self.rpc("heartbeat", json!({"prev_rtt_ms": prev_rtt_ms}))
            .await
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
