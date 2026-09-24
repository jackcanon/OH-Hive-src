use super::*;
#[cfg(feature = "bots")]
use crate::bots::*;
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
        "private_coding_pending" => wire(h.private_coding_pending()?),
        "private_run_retry" => {
            wire(h.private_run_retry(argument(p, "previous")?, argument(p, "next")?)?)
        }
        "private_run_ready" => wire(h.private_run_ready(argument(p, "operation")?)?),
        "private_run_stop" => wire(h.private_run_stop(argument(p, "operation")?)?),
        "private_run_work" => wire(h.private_run_work(argument(p, "operation")?)?),
        "private_run_request" => {
            wire(h.private_run_request(argument(p, "operation")?, argument(p, "task")?)?)
        }
        "private_run_status" => wire(h.private_run_status(argument(p, "operation")?)?),
        "private_run_claim" => wire(h.private_run_claim(argument(p, "operation")?).await?),
        "private_coding_advertise" => wire(h.private_coding_advertise(&argument(p, "report")?)?),
        "private_coding_hosts" => wire(h.private_coding_hosts()?),
        "private_preparation_recover" => {
            wire(h.private_preparation_recover(argument(p, "request")?, argument(p, "operation")?)?)
        }
        "private_preparation_request" => {
            wire(h.private_preparation_request(argument(p, "operation")?, argument(p, "task")?)?)
        }
        "private_preparation_status" => {
            wire(h.private_preparation_status(argument(p, "operation")?)?)
        }
        "private_preparation_take" => wire(h.private_preparation_take()?),
        "private_preparation_complete" => wire(h.private_preparation_complete(
            argument(p, "operation")?,
            &argument::<String>(p, "workspace")?,
        )?),
        "private_execution_hosts" => wire(h.private_execution_hosts()?),
        "private_code_task_stage" => wire(h.private_code_task_stage(&argument(p, "request")?)?),
        "private_fleet_identity" => wire(h.private_fleet_identity()?),
        #[cfg(feature = "bots")]
        "bots_agent_tool_settings" => wire(h.bots_agent_tool_settings(argument(p, "agent")?)?),
        #[cfg(feature = "bots")]
        "bots_agent_tool_policy_get" => wire(h.bots_agent_tool_policy_get(argument(p, "agent")?)?),
        #[cfg(feature = "bots")]
        "bots_agent_tool_policy_set" => {
            wire(h.bots_agent_tool_policy_set(argument(p, "agent")?, argument(p, "policy")?)?)
        }
        #[cfg(feature = "bots")]
        "bots_agent_web_authorize" => wire(h.bots_agent_web_authorize(
            argument(p, "agent")?,
            argument(p, "revision")?,
            &argument(p, "turn")?,
            &argument::<String>(p, "url")?,
        )?),
        "bots_agent_tool_execute" => wire(h.bots_agent_tool_execute(
            argument(p, "agent")?,
            argument(p, "revision")?,
            &argument(p, "turn")?,
            argument(p, "call")?,
        )?),
        "enrollment_challenge" => wire(h.enrollment_challenge()?),
        "enrollment_complete" => wire(h.enrollment_complete(argument(p, "assertion")?)?),
        #[cfg(feature = "bots")]
        "bots_message_get" => wire(h.bots_message_get(argument(p, "id")?)?),
        #[cfg(feature = "bots")]
        "bots_conversation_deliveries" => {
            wire(h.bots_conversation_deliveries(argument(p, "conversation")?)?)
        }
        #[cfg(feature = "bots")]
        "bots_agent_was_archived" => {
            wire(h.bots_agent_was_archived(argument(p, "runtime")?, argument(p, "host")?)?)
        }
        "bots_agent_bio_get" => wire(h.bots_agent_bio_get(argument(p, "agent")?)?),
        "bots_agent_bio_set" => wire(h.bots_agent_bio_set(
            argument(p, "agent")?,
            argument(p, "name")?,
            argument(p, "profile")?,
        )?),
        "bots_user_profile_get" => wire(h.bots_user_profile_get()?),
        #[cfg(feature = "bots")]
        "bots_user_profile_set" => wire(h.bots_user_profile_set(argument(p, "profile")?)?),
        #[cfg(feature = "bots")]
        "bots_agents_list" => wire(h.bots_agents_list()?),
        #[cfg(feature = "bots")]
        "bots_agents_create" => wire(h.bots_agents_create(argument(p, "draft")?)?),
        #[cfg(feature = "bots")]
        "bots_agents_update" => {
            wire(h.bots_agents_update(argument(p, "agent_id")?, argument(p, "patch")?)?)
        }
        #[cfg(feature = "bots")]
        "bots_agents_archive" => wire(h.bots_agents_archive(argument(p, "agent_id")?)?),
        #[cfg(feature = "bots")]
        "bots_room_agents" => wire(h.bots_room_agents(argument(p, "conversation_id")?)?),
        #[cfg(feature = "bots")]
        "bots_conversations_list" => wire(h.bots_conversations_list(argument(p, "actor")?)?),
        #[cfg(feature = "bots")]
        "bots_rooms_create" => wire(h.bots_rooms_create(
            argument(p, "request_id")?,
            argument(p, "draft")?,
            argument(p, "agents")?,
        )?),
        "bots_conversations_create" => wire(h.bots_conversations_create(argument(p, "draft")?)?),
        #[cfg(feature = "bots")]
        "bots_conversations_join" => {
            wire(h.bots_conversations_join(argument(p, "actor")?, argument(p, "conversation_id")?)?)
        }
        #[cfg(feature = "bots")]
        "bots_messages_list" => wire(h.bots_messages_list(
            argument(p, "actor")?,
            argument(p, "conversation_id")?,
            argument(p, "page")?,
        )?),
        #[cfg(feature = "bots")]
        "bots_message_send" => wire(h.bots_message_send(
            argument(p, "actor")?,
            argument(p, "conversation_id")?,
            argument(p, "client_request_id")?,
            argument(p, "expected_policy_revision")?,
            argument(p, "recipient_ids")?,
            argument(p, "draft")?,
        )?),
        #[cfg(feature = "bots")]
        "bots_conversation_mark_read" => wire(h.bots_conversation_mark_read(
            argument(p, "conversation_id")?,
            argument(p, "up_to_sequence")?,
        )?),
        // Track E item 4: the delivery surface, so a second machine's `DeliveryExecutor` can run
        // its own agents against this hub. Every one of these dispatches to a `LocalHub` wrapper
        // that resolves the host from `h`'s own session key -- NOT from `p`. Wiring any of these
        // to the same-named `LocalHubStore` method instead would hand a paired node the ability
        // to claim another machine's agent deliveries and answer as it. See the Track E item 4
        // block in `bots.rs` for the full reasoning.
        #[cfg(feature = "bots")]
        "bots_delivery_claim" => wire(h.bots_delivery_claim(argument(p, "delivery_key")?)?),
        #[cfg(feature = "bots")]
        "bots_delivery_complete" => wire(h.bots_delivery_complete(
            argument(p, "delivery_key")?,
            argument(p, "lease_generation")?,
        )?),
        #[cfg(feature = "bots")]
        "bots_delivery_fail" => wire(h.bots_delivery_fail(
            argument(p, "delivery_key")?,
            argument(p, "lease_generation")?,
            argument(p, "retry_after")?,
        )?),
        #[cfg(feature = "bots")]
        "bots_message_send_with_cause" => wire(h.bots_message_send_with_cause(
            argument(p, "actor")?,
            argument(p, "conversation_id")?,
            argument(p, "client_request_id")?,
            argument(p, "expected_policy_revision")?,
            argument(p, "recipient_ids")?,
            argument(p, "draft")?,
            argument(p, "cause")?,
            argument(p, "hold")?,
        )?),
        #[cfg(feature = "bots")]
        "bots_turns_for_root" => wire(h.bots_turns_for_root(argument(p, "root_message_id")?)?),
        #[cfg(feature = "bots")]
        "bots_deliveries_pending_for_agent" => wire(
            h.bots_deliveries_pending_for_agent(argument(p, "agent_id")?, argument(p, "limit")?)?,
        ),
        // `local_ready` only. owner and host come from the session -- see the wrapper's doc for
        // why letting a caller name the host would let it write off another machine's agents.
        #[cfg(feature = "bots")]
        "bots_report_unroutable" => wire(h.bots_report_unroutable(argument(p, "local_ready")?)?),
        #[cfg(feature = "bots")]
        "bots_deliveries_release_root" => {
            wire(h.bots_deliveries_release_root(argument(p, "root_message_id")?)?)
        }
        #[cfg(feature = "bots")]
        "bots_active_turns_for_agent" => {
            wire(h.bots_active_turns_for_agent(argument(p, "agent_id")?)?)
        }
        "vault_list" => wire(h.vault_list()?),
        "vault_status" => wire(h.vault_status(argument(p, "vault_id")?)?),
        "vault_search" => wire(h.vault_search(
            argument(p, "vault_id")?,
            &argument::<String>(p, "query")?,
            argument(p, "limit")?,
        )?),
        "vault_read" => wire(h.vault_read(
            argument(p, "vault_id")?,
            argument(p, "document_id")?,
            &argument::<String>(p, "revision")?,
        )?),
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
    run_operation: Option<Uuid>,
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
/// Why the call failed, not merely that it did. `map_err(|_| "local hub unreachable")` was
/// accurate and useless in the same breath: a hub that is down, a connection that died
/// mid-flight, and a pooled connection gone stale after a hub restart are three different
/// problems with three different fixes, and they all printed that one sentence. reqwest keeps
/// the real cause in the error's `source` chain, which is where the distinguishing detail
/// ("connection closed before message completed", "tcp connect error", ...) actually lives, so
/// walk it rather than taking the top-level `Display` alone.
fn transport_error(method: &str, e: reqwest::Error) -> HubError {
    let mut detail = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(cause) = source {
        detail.push_str(": ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    HubError::Transport(format!("local hub unreachable calling {method}: {detail}"))
}

impl RemoteLocalHub {
    pub async fn private_coding_advertise(
        &self,
        report: &super::private_readiness::CodingReadiness,
    ) -> Result<()> {
        self.rpc("private_coding_advertise", json!({"report":report}))
            .await
    }
    pub async fn private_coding_hosts(&self) -> Result<Vec<super::private_readiness::CodingHost>> {
        self.rpc("private_coding_hosts", json!({})).await
    }

    pub async fn private_coding_pending(
        &self,
    ) -> Result<super::private_dispatch::CodingPendingWork> {
        self.rpc("private_coding_pending", json!({})).await
    }

    pub async fn private_preparation_recover(
        &self,
        request: Uuid,
        operation: Uuid,
    ) -> Result<super::private_preparation::PreparationStatus> {
        self.rpc(
            "private_preparation_recover",
            json!({"request":request,"operation":operation}),
        )
        .await
    }

    pub async fn private_run_retry(
        &self,
        previous: Uuid,
        next: Uuid,
    ) -> Result<super::private_run::PrivateRunStatus> {
        self.rpc(
            "private_run_retry",
            json!({"previous":previous,"next":next}),
        )
        .await
    }
    pub async fn private_run_ready(
        &self,
        operation: Uuid,
    ) -> Result<super::private_run::PrivateRunStatus> {
        self.rpc("private_run_ready", json!({"operation":operation}))
            .await
    }

    pub async fn private_run_stop(
        &self,
        operation: Uuid,
    ) -> Result<super::private_run::PrivateRunStatus> {
        self.rpc("private_run_stop", json!({"operation":operation}))
            .await
    }
    pub async fn private_run_work(&self, operation: Uuid) -> Result<ClaimedCard> {
        self.rpc("private_run_work", json!({"operation":operation}))
            .await
    }

    pub fn for_private_run(mut self, operation: Uuid) -> Self {
        self.run_operation = Some(operation);
        self.session = Uuid::new_v4();
        self
    }
    pub async fn private_run_request(
        &self,
        operation: Uuid,
        task: Uuid,
    ) -> Result<super::private_run::PrivateRunStatus> {
        self.rpc(
            "private_run_request",
            json!({"operation":operation,"task":task}),
        )
        .await
    }
    pub async fn private_run_status(
        &self,
        operation: Uuid,
    ) -> Result<super::private_run::PrivateRunStatus> {
        self.rpc("private_run_status", json!({"operation":operation}))
            .await
    }

    pub async fn private_preparation_request(
        &self,
        operation: Uuid,
        task: Uuid,
    ) -> Result<super::private_preparation::PreparationStatus> {
        self.rpc(
            "private_preparation_request",
            json!({"operation":operation,"task":task}),
        )
        .await
    }
    pub async fn private_preparation_status(
        &self,
        operation: Uuid,
    ) -> Result<super::private_preparation::PreparationStatus> {
        self.rpc("private_preparation_status", json!({"operation":operation}))
            .await
    }
    pub async fn private_preparation_take(
        &self,
    ) -> Result<Option<super::private_preparation::PreparationWork>> {
        self.rpc("private_preparation_take", json!({})).await
    }
    pub async fn private_preparation_complete(
        &self,
        operation: Uuid,
        workspace: &str,
    ) -> Result<super::private_preparation::PreparationStatus> {
        self.rpc(
            "private_preparation_complete",
            json!({"operation":operation,"workspace":workspace}),
        )
        .await
    }

    pub async fn private_execution_hosts(
        &self,
    ) -> Result<Vec<super::private_code_tasks::PrivateExecutionHost>> {
        self.rpc("private_execution_hosts", json!({})).await
    }
    pub async fn private_code_task_stage(
        &self,
        request: &super::private_code_tasks::PrivateCodeTaskRequest,
    ) -> Result<ClaimedCard> {
        self.rpc("private_code_task_stage", json!({"request":request}))
            .await
    }

    #[cfg(feature = "bots")]
    pub async fn bots_message_get(&self, id: Uuid) -> Result<crate::bots::Message> {
        self.rpc("bots_message_get", json!({"id": id})).await
    }
    pub async fn private_fleet_identity(
        &self,
    ) -> Result<Option<super::enrollment::EnrollmentReceipt>> {
        self.rpc("private_fleet_identity", json!({})).await
    }
    pub async fn enrollment_challenge(&self) -> Result<super::enrollment::EnrollmentChallenge> {
        self.rpc("enrollment_challenge", json!({})).await
    }
    pub async fn enrollment_complete(
        &self,
        assertion: super::enrollment::EnrollmentAssertion,
    ) -> Result<super::enrollment::EnrollmentReceipt> {
        self.rpc("enrollment_complete", json!({"assertion": assertion}))
            .await
    }

    pub async fn vault_list(&self) -> Result<Vec<super::vault::VaultInfo>> {
        self.rpc("vault_list", json!({})).await
    }
    pub async fn vault_status(&self, vault_id: Uuid) -> Result<super::vault::VaultInfo> {
        self.rpc("vault_status", json!({"vault_id":vault_id})).await
    }
    pub async fn vault_search(
        &self,
        vault_id: Uuid,
        query: &str,
        limit: u32,
    ) -> Result<Vec<super::vault::VaultHit>> {
        self.rpc(
            "vault_search",
            json!({"vault_id":vault_id,"query":query,"limit":limit}),
        )
        .await
    }
    pub async fn vault_read(
        &self,
        vault_id: Uuid,
        document_id: Uuid,
        revision: &str,
    ) -> Result<super::vault::VaultDocument> {
        self.rpc(
            "vault_read",
            json!({"vault_id":vault_id,"document_id":document_id,"revision":revision}),
        )
        .await
    }

    pub fn new(base: &str, key: String) -> Result<Self> {
        let (base, http) = client(base)?;
        Ok(Self {
            base,
            key,
            session: Uuid::new_v4(),
            run_operation: None,
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
        let send = || {
            self.http
                .post(format!("{}/local/v1/rpc", self.base))
                .bearer_auth(&self.key)
                .json(&json!({"session":self.session,"method":method,"params":params}))
                .send()
        };
        let r = match send().await {
            Ok(r) => r,
            // Retry once, and ONLY when the connection was never established: in that case the
            // request bytes provably never reached the hub, so replaying them cannot apply
            // anything twice. Every other transport failure is left alone -- above all a
            // connection that died *after* the request went out, where the hub may well have
            // applied it. `bots_message_send` is not idempotent, and a well-meant retry there
            // posts the message twice.
            //
            // The case this buys is the ordinary one: the hub restarts (upgrade, reboot, crash)
            // and a worker mid-poll finds the socket gone. Before this, that worker reported
            // "unreachable" and kept reporting it, once per poll, until someone restarted it by
            // hand -- for a hub that had been back for minutes.
            Err(e) if e.is_connect() => send().await.map_err(|e| transport_error(method, e))?,
            Err(e) => return Err(transport_error(method, e)),
        };
        if r.status() == StatusCode::UNAUTHORIZED {
            return Err(HubError::BadKey);
        }
        if r.status() == StatusCode::SERVICE_UNAVAILABLE {
            return Err(HubError::Transport("local hub or vault unavailable".into()));
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
        if let Some(operation) = self.run_operation {
            self.rpc("private_run_claim", json!({"operation":operation}))
                .await
        } else {
            self.rpc("claim_card", json!({})).await
        }
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

#[cfg(feature = "bots")]
impl RemoteLocalHub {
    pub async fn bots_agent_tool_settings(&self, agent: Uuid) -> Result<Value> {
        self.rpc("bots_agent_tool_settings", json!({"agent":agent}))
            .await
    }
    #[cfg(feature = "bots")]
    pub async fn bots_agent_tool_policy_get(
        &self,
        agent: Uuid,
    ) -> Result<super::agent_tools::AgentToolPolicy> {
        self.rpc("bots_agent_tool_policy_get", json!({"agent":agent}))
            .await
    }
    pub async fn bots_agent_web_authorize(
        &self,
        agent: Uuid,
        revision: u32,
        turn: &super::agent_tools::AgentToolTurn,
        url: &str,
    ) -> Result<super::agent_tools::WebFetchGrant> {
        self.rpc(
            "bots_agent_web_authorize",
            json!({"agent":agent,"revision":revision,"turn":turn,"url":url}),
        )
        .await
    }
    pub async fn bots_agent_tool_policy_set(
        &self,
        agent: Uuid,
        policy: super::agent_tools::AgentToolPolicy,
    ) -> Result<super::agent_tools::AgentToolPolicy> {
        self.rpc(
            "bots_agent_tool_policy_set",
            json!({"agent":agent,"policy":policy}),
        )
        .await
    }
    pub async fn bots_agent_tool_execute(
        &self,
        agent: Uuid,
        revision: u32,
        turn: &super::agent_tools::AgentToolTurn,
        call: super::agent_tools::AgentToolCall,
    ) -> Result<Value> {
        self.rpc(
            "bots_agent_tool_execute",
            json!({"agent":agent,"revision":revision,"turn":turn,"call":call}),
        )
        .await
    }

    pub async fn bots_conversation_deliveries(
        &self,
        conversation: Uuid,
    ) -> Result<Vec<(String, String, String)>> {
        self.rpc(
            "bots_conversation_deliveries",
            json!({"conversation":conversation}),
        )
        .await
    }
    pub async fn bots_agent_was_archived(
        &self,
        runtime: String,
        host: Option<uuid::Uuid>,
    ) -> Result<bool> {
        self.rpc(
            "bots_agent_was_archived",
            json!({"runtime":runtime,"host":host}),
        )
        .await
    }
    pub async fn bots_agent_bio_get(&self, agent: uuid::Uuid) -> Result<crate::bots::AgentBio> {
        self.rpc("bots_agent_bio_get", json!({"agent":agent})).await
    }
    pub async fn bots_agent_bio_set(
        &self,
        agent: uuid::Uuid,
        name: String,
        profile: crate::bots::AgentBio,
    ) -> Result<crate::bots::AgentBio> {
        self.rpc(
            "bots_agent_bio_set",
            json!({"agent":agent,"name":name,"profile":profile}),
        )
        .await
    }
    pub async fn bots_user_profile_get(&self) -> Result<crate::bots::UserProfile> {
        self.rpc("bots_user_profile_get", json!({})).await
    }
    pub async fn bots_user_profile_set(
        &self,
        profile: crate::bots::UserProfile,
    ) -> Result<crate::bots::UserProfile> {
        self.rpc("bots_user_profile_set", json!({"profile":profile}))
            .await
    }
    pub async fn bots_agents_list(&self) -> Result<Vec<AgentProfile>> {
        self.rpc("bots_agents_list", json!({})).await
    }
    pub async fn bots_agents_create(&self, draft: NewAgentProfile) -> Result<AgentProfile> {
        self.rpc("bots_agents_create", json!({"draft": draft}))
            .await
    }
    pub async fn bots_agents_update(
        &self,
        agent_id: AgentId,
        patch: AgentProfilePatch,
    ) -> Result<AgentProfile> {
        self.rpc(
            "bots_agents_update",
            json!({"agent_id": agent_id, "patch": patch}),
        )
        .await
    }
    pub async fn bots_agents_archive(&self, agent_id: AgentId) -> Result<()> {
        self.rpc("bots_agents_archive", json!({"agent_id": agent_id}))
            .await
    }
    #[cfg(feature = "bots")]
    pub async fn bots_room_agents(&self, conversation_id: Uuid) -> Result<Vec<AgentProfile>> {
        self.rpc(
            "bots_room_agents",
            json!({"conversation_id": conversation_id}),
        )
        .await
    }
    #[cfg(feature = "bots")]
    pub async fn bots_conversations_list(&self, actor: Principal) -> Result<Vec<Conversation>> {
        self.rpc("bots_conversations_list", json!({"actor": actor}))
            .await
    }
    pub async fn bots_rooms_create(
        &self,
        request_id: Uuid,
        draft: NewConversation,
        agents: Vec<AgentId>,
    ) -> Result<Conversation> {
        self.rpc(
            "bots_rooms_create",
            json!({"request_id":request_id,"draft":draft,"agents":agents}),
        )
        .await
    }
    pub async fn bots_conversations_create(&self, draft: NewConversation) -> Result<Conversation> {
        self.rpc("bots_conversations_create", json!({"draft": draft}))
            .await
    }
    pub async fn bots_conversations_join(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
    ) -> Result<ConversationMember> {
        self.rpc(
            "bots_conversations_join",
            json!({"actor": actor, "conversation_id": conversation_id}),
        )
        .await
    }
    pub async fn bots_messages_list(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        page: MessagePage,
    ) -> Result<Vec<Message>> {
        self.rpc(
            "bots_messages_list",
            json!({"actor": actor, "conversation_id": conversation_id, "page": page}),
        )
        .await
    }
    pub async fn bots_message_send(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
    ) -> Result<Message> {
        self.rpc("bots_message_send", json!({"actor": actor, "conversation_id": conversation_id, "client_request_id": client_request_id, "expected_policy_revision": expected_policy_revision, "recipient_ids": recipient_ids, "draft": draft})).await
    }
    pub async fn bots_conversation_mark_read(
        &self,
        conversation_id: ConversationId,
        up_to_sequence: u64,
    ) -> Result<ConversationReadPosition> {
        self.rpc(
            "bots_conversation_mark_read",
            json!({"conversation_id": conversation_id, "up_to_sequence": up_to_sequence}),
        )
        .await
    }

    // Track E item 4 client half. Note what is NOT sent on any of these: a node id. The host is
    // whatever the bearer key resolves to server-side, so a tampered client cannot widen its own
    // authority by lying about who it is -- the worst it can do is ask for a delivery it does not
    // host and be refused.
    pub async fn bots_delivery_claim(&self, delivery_key: DeliveryKey) -> Result<AgentDelivery> {
        self.rpc("bots_delivery_claim", json!({"delivery_key": delivery_key}))
            .await
    }
    pub async fn bots_delivery_complete(
        &self,
        delivery_key: DeliveryKey,
        lease_generation: u64,
    ) -> Result<AgentDelivery> {
        self.rpc(
            "bots_delivery_complete",
            json!({"delivery_key": delivery_key, "lease_generation": lease_generation}),
        )
        .await
    }
    pub async fn bots_delivery_fail(
        &self,
        delivery_key: DeliveryKey,
        lease_generation: u64,
        retry_after: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<AgentDelivery> {
        self.rpc(
            "bots_delivery_fail",
            json!({"delivery_key": delivery_key, "lease_generation": lease_generation, "retry_after": retry_after}),
        )
        .await
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn bots_message_send_with_cause(
        &self,
        actor: Principal,
        conversation_id: ConversationId,
        client_request_id: String,
        expected_policy_revision: u32,
        recipient_ids: Vec<AgentId>,
        draft: NewMessage,
        cause: Option<DeliveryCause>,
        hold: bool,
    ) -> Result<Message> {
        self.rpc("bots_message_send_with_cause", json!({"actor": actor, "conversation_id": conversation_id, "client_request_id": client_request_id, "expected_policy_revision": expected_policy_revision, "recipient_ids": recipient_ids, "draft": draft, "cause": cause, "hold": hold})).await
    }
    pub async fn bots_turns_for_root(&self, root_message_id: MessageId) -> Result<u32> {
        self.rpc(
            "bots_turns_for_root",
            json!({"root_message_id": root_message_id}),
        )
        .await
    }
    pub async fn bots_deliveries_pending_for_agent(
        &self,
        agent_id: AgentId,
        limit: u32,
    ) -> Result<Vec<AgentDelivery>> {
        self.rpc(
            "bots_deliveries_pending_for_agent",
            json!({"agent_id": agent_id, "limit": limit}),
        )
        .await
    }
    pub async fn bots_report_unroutable(&self, local_ready: bool) -> Result<usize> {
        self.rpc(
            "bots_report_unroutable",
            json!({"local_ready": local_ready}),
        )
        .await
    }
    pub async fn bots_deliveries_release_root(&self, root_message_id: MessageId) -> Result<u32> {
        self.rpc(
            "bots_deliveries_release_root",
            json!({"root_message_id": root_message_id}),
        )
        .await
    }
    pub async fn bots_active_turns_for_agent(&self, agent_id: AgentId) -> Result<u32> {
        self.rpc("bots_active_turns_for_agent", json!({"agent_id": agent_id}))
            .await
    }
}
