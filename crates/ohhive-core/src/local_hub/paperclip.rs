//! Paperclip heartbeat bridge (spike): `POST /local/v1/paperclip/heartbeat`.
//!
//! Paperclip's "http" adapter POSTs a heartbeat here. We turn it into an ordinary Bots turn:
//! one message from the conversation owner, addressed to the seat's Den agent, through
//! `bots_message_send`. The agent's host runner claims the delivery and answers exactly as it
//! would for a human, so every tool call still goes through hub authorize and lands in
//! `bots_agent_tool_receipts`, and the loop-safety budgets still apply. Nothing here runs a
//! model or touches the vault schema.
//!
//! Auth is a per-seat bearer secret (never a node key or the vault key). Seats live in a JSON
//! file re-read on every request (`OHHIVE_PAPERCLIP_SEATS`, default
//! `<config dir>/ohhive/paperclip-seats.json`), keyed by Paperclip agent id:
//! `{"<agentId>": {"den_agent_id": uuid, "conversation_id": uuid, "secret_sha256": hex,
//! "paperclip_base_url": "http://127.0.0.1:3100"}}`.
//!
//! Reply detection (conservative): the first `Text` message authored by the Den agent in the
//! seat's conversation with a sequence after the one we sent. Intermediate agent chatter would
//! be taken as the reply; tool receipts are `TaskReceipt` kind and are ignored.
//!
//! Cost events: NOT posted. The hub's message rows carry no token usage (the runner sees
//! `prompt_tokens`/`completion_tokens` in-process but does not persist them), and Paperclip's
//! cost-event body also needs provider/model/costCents which we cannot know here.
//!
//! Retry note: a repeated `runId` re-sends nothing (idempotent `client_request_id`) and returns
//! the existing reply if present, but will PATCH the Paperclip ticket again (status `in_review`
//! plus comment, see `post_reply`). That PATCH does not re-wake the agent, so retries are safe.
use super::*;
use crate::bots::*;
use axum::{body::Bytes, extract::State, http::HeaderMap, http::StatusCode, Json};
use std::{
    collections::HashMap,
    path::{Path as FsPath, PathBuf},
    time::{Duration, Instant},
};

const MAX_BODY: usize = 64 * 1024;
const MAX_TICKET_TEXT: usize = 32 * 1024;
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;
const POLL: Duration = Duration::from_millis(500);
const DEFAULT_PAPERCLIP: &str = "http://127.0.0.1:3100";

#[derive(Deserialize)]
struct Seat {
    den_agent_id: Uuid,
    conversation_id: Uuid,
    secret_sha256: String,
    #[serde(default)]
    paperclip_base_url: Option<String>,
}

#[derive(Debug, PartialEq)]
struct Heartbeat {
    run_id: String,
    task_id: Uuid,
    comment_id: Option<Uuid>,
    wake_reason: Option<String>,
    timeout: Duration,
}

pub(super) fn seats_path() -> PathBuf {
    if let Some(p) = std::env::var_os("OHHIVE_PAPERCLIP_SEATS") {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ohhive")
        .join("paperclip-seats.json")
}

fn load_seats(path: &FsPath) -> Option<HashMap<String, Seat>> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Constant-time comparison; unequal length is a plain mismatch (lengths are not secret: both
/// sides are fixed-size hex digests).
fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn secret_matches(seat: &Seat, presented: &str) -> bool {
    ct_eq(
        &digest(presented),
        &seat.secret_sha256.trim().to_ascii_lowercase(),
    )
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

fn reply(code: StatusCode, status: &str, reason: &str) -> (StatusCode, Json<Value>) {
    (code, Json(json!({"status": status, "reason": reason})))
}

fn uuid_field(v: &Value, k: &str) -> Option<Uuid> {
    v.get(k)?.as_str()?.parse().ok()
}

fn parse_heartbeat(v: &Value) -> Result<Heartbeat, &'static str> {
    let run_id = v.get("runId").and_then(Value::as_str).unwrap_or("");
    if run_id.is_empty() || run_id.len() > 128 || run_id.chars().any(char::is_control) {
        return Err("invalid runId");
    }
    // Paperclip's http adapter body is `{agentId, runId, context, ..payloadTemplate}` -- it does NOT
    // send companyId (the handoff doc said it did; observed against v2026.916.1). Optional here, but
    // if a caller does send one it must be well formed.
    if matches!(v.get("companyId"), Some(c) if !c.is_null()) {
        uuid_field(v, "companyId").ok_or("invalid companyId")?;
    }
    let ctx = v.get("context").ok_or("missing context")?;
    let task_id = uuid_field(ctx, "taskId").ok_or("invalid context.taskId")?;
    let comment_id = match ctx.get("commentId") {
        None | Some(Value::Null) => None,
        Some(_) => Some(uuid_field(ctx, "commentId").ok_or("invalid context.commentId")?),
    };
    let wake_reason = ctx
        .get("wakeReason")
        .and_then(Value::as_str)
        .map(|s| s.chars().filter(|c| !c.is_control()).take(64).collect());
    let secs = match v.get("timeoutSec") {
        None | Some(Value::Null) => DEFAULT_TIMEOUT_SECS,
        Some(t) => t.as_u64().ok_or("invalid timeoutSec")?,
    };
    Ok(Heartbeat {
        run_id: run_id.into(),
        task_id,
        comment_id,
        wake_reason,
        timeout: Duration::from_secs(secs.clamp(1, MAX_TIMEOUT_SECS)),
    })
}

fn clip(s: &str) -> String {
    if s.len() <= MAX_TICKET_TEXT {
        return s.to_owned();
    }
    let mut end = MAX_TICKET_TEXT;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

fn base_url(seat: &Seat) -> Result<String, ()> {
    let u = seat
        .paperclip_base_url
        .clone()
        .or_else(|| std::env::var("OHHIVE_PAPERCLIP_URL").ok())
        .unwrap_or_else(|| DEFAULT_PAPERCLIP.into());
    let u = u.trim_end_matches('/').to_owned();
    if u.starts_with("http://") || u.starts_with("https://") {
        Ok(u)
    } else {
        Err(())
    }
}

struct Paperclip {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
}

impl Paperclip {
    fn req(&self, m: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let r = self.http.request(m, format!("{}{path}", self.base));
        match &self.token {
            Some(t) => r.bearer_auth(t),
            None => r,
        }
    }
    async fn get(&self, path: &str) -> Result<Value, ()> {
        let r = self
            .req(reqwest::Method::GET, path)
            .send()
            .await
            .map_err(|_| ())?;
        if !r.status().is_success() {
            return Err(());
        }
        r.json().await.map_err(|_| ())
    }
    /// Ticket title/description plus the triggering comment, as one clipped prompt. Ticket text
    /// is never logged.
    async fn ticket_prompt(&self, hb: &Heartbeat) -> Result<String, ()> {
        let issue = self.get(&format!("/api/issues/{}", hb.task_id)).await?;
        let text = |k| {
            issue
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        let mut out = format!(
            "Paperclip ticket (wake reason: {})\nTitle: {}\n\n{}",
            hb.wake_reason.as_deref().unwrap_or("unspecified"),
            text("title"),
            text("description")
        );
        if let Some(cid) = hb.comment_id {
            let comments = self
                .get(&format!("/api/issues/{}/comments", hb.task_id))
                .await?;
            let list = comments
                .as_array()
                .or_else(|| comments.get("comments").and_then(Value::as_array))
                .ok_or(())?;
            let want = cid.to_string();
            let body = list
                .iter()
                .find(|c| c.get("id").and_then(Value::as_str) == Some(want.as_str()))
                .and_then(|c| c.get("body").and_then(Value::as_str))
                .ok_or(())?;
            out.push_str("\n\nNew comment:\n");
            out.push_str(body);
        }
        out.push_str(
            "\n\nReply with your response to this ticket; it will be posted as a comment.",
        );
        Ok(clip(&out))
    }
    /// Post our reply via a status PATCH, not the comments endpoint. Paperclip treats *any*
    /// new comment -- including one we just posted ourselves -- as a fresh inbound signal and
    /// re-wakes the assignee, which built a real wake loop in production (2026-09-24: a seat's
    /// own reply comment re-triggered its heartbeat, over and over, dozens of runs in minutes).
    /// `PATCH /api/issues/{id}` with `{status, comment}` sets the ticket to `in_review` and
    /// attaches our answer as the same-request comment; verified against a live instance that
    /// this does NOT re-trigger a wake, unlike posting to /comments. `in_review` leaves the
    /// ticket clearly "answered, needs a look" without a live human/agent comment reopening it.
    async fn post_reply(&self, task: Uuid, body: &str) -> Result<(), ()> {
        let r = self
            .req(reqwest::Method::PATCH, &format!("/api/issues/{task}"))
            .json(&json!({"status": "in_review", "comment": body}))
            .send()
            .await
            .map_err(|_| ())?;
        r.status().is_success().then_some(()).ok_or(())
    }
}

/// Everything after routing, with the seats file and poll interval injected so tests do not
/// depend on process environment.
async fn handle(
    store: LocalHubStore,
    seats_file: &FsPath,
    poll: Duration,
    headers: &HeaderMap,
    body: &[u8],
) -> (StatusCode, Json<Value>) {
    let unauthorized = || reply(StatusCode::UNAUTHORIZED, "unauthorized", "");
    if body.len() > MAX_BODY {
        return reply(StatusCode::PAYLOAD_TOO_LARGE, "error", "body too large");
    }
    let Ok(payload) = serde_json::from_slice::<Value>(body) else {
        return reply(StatusCode::BAD_REQUEST, "error", "invalid json");
    };
    // Authenticate before validating anything else, and give no detail on failure.
    let seats = load_seats(seats_file);
    let agent_key = payload.get("agentId").and_then(Value::as_str).unwrap_or("");
    let presented = bearer(headers).unwrap_or("");
    let seat = seats.as_ref().and_then(|s| s.get(agent_key));
    // Always hash and compare so unknown seats cost about the same as wrong secrets.
    let ok = match seat {
        Some(s) => secret_matches(s, presented),
        None => {
            let _ = ct_eq(&digest(presented), &digest(""));
            false
        }
    };
    let (Some(seat), true) = (seat, ok && !presented.is_empty()) else {
        return unauthorized();
    };
    let hb = match parse_heartbeat(&payload) {
        Ok(h) => h,
        Err(e) => return reply(StatusCode::BAD_REQUEST, "error", e),
    };
    let Ok(base) = base_url(seat) else {
        return reply(
            StatusCode::BAD_GATEWAY,
            "error",
            "paperclip url misconfigured",
        );
    };
    let Ok(http) = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    else {
        return reply(StatusCode::BAD_GATEWAY, "error", "http client");
    };
    let pc = Paperclip {
        http,
        base,
        token: std::env::var("OHHIVE_PAPERCLIP_TOKEN")
            .ok()
            .filter(|t| !t.is_empty()),
    };
    let Ok(prompt) = pc.ticket_prompt(&hb).await else {
        return reply(
            StatusCode::BAD_GATEWAY,
            "error",
            "paperclip ticket fetch failed",
        );
    };
    let started = Instant::now();
    let (den, conv) = (seat.den_agent_id, seat.conversation_id);

    // Send as the conversation's owner, addressed to the Den agent. Blocking sqlite work stays
    // off the executor.
    let s = store.clone();
    let key = format!("paperclip:{}", hb.run_id);
    let sent = tokio::task::spawn_blocking(move || {
        let c = s.bots_conversation_get(conv)?;
        s.bots_message_send(
            Principal::User(c.owner),
            conv,
            key,
            c.policy_revision,
            vec![den],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some(prompt),
                attachment_refs: vec![],
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .map(|m| (c.owner, m))
    })
    .await;
    let (owner, sent) = match sent {
        Ok(Ok(v)) => v,
        Ok(Err(_)) => {
            return reply(StatusCode::CONFLICT, "error", "hub rejected the message");
        }
        Err(_) => {
            return reply(
                StatusCode::INTERNAL_SERVER_ERROR,
                "error",
                "hub task failed",
            );
        }
    };

    // Poll for the agent's reply.
    let deadline = Instant::now() + hb.timeout;
    let mut after = sent.server_sequence;
    let answer = loop {
        let s = store.clone();
        let page = tokio::task::spawn_blocking(move || {
            s.bots_messages_list(
                Principal::User(owner),
                conv,
                MessagePage {
                    before: None,
                    after: Some(after),
                    limit: 100,
                },
            )
        })
        .await;
        match page {
            Ok(Ok(msgs)) => {
                if let Some(m) = msgs.iter().find(|m| {
                    m.author == Principal::Agent(den)
                        && m.kind == MessageKind::Text
                        && m.body.as_deref().is_some_and(|b| !b.trim().is_empty())
                }) {
                    break m.clone();
                }
                if let Some(last) = msgs.last() {
                    after = last.server_sequence;
                }
            }
            _ => {
                return reply(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "error",
                    "hub read failed",
                );
            }
        }
        // Fail fast if the runner already gave up on our delivery: otherwise a failed turn (seen
        // 2026-09-24 with Hel: model answered, delivery went `failed`, no message) would burn the
        // whole timeout before Paperclip hears about it.
        let s = store.clone();
        let msg_id = sent.id.to_string();
        let dead = tokio::task::spawn_blocking(move || {
            s.bots_conversation_deliveries(owner, conv).map(|rows| {
                rows.into_iter()
                    .find(|(m, _, st)| {
                        *m == msg_id && matches!(st.as_str(), "failed" | "cancelled" | "unknown")
                    })
                    .map(|(_, _, st)| st)
            })
        })
        .await;
        if let Ok(Ok(Some(status))) = dead {
            return reply(
                StatusCode::BAD_GATEWAY,
                "delivery_failed",
                match status.as_str() {
                    "cancelled" => "agent delivery was cancelled",
                    "unknown" => "agent delivery state lost",
                    _ => "agent turn failed",
                },
            );
        }
        if Instant::now() >= deadline {
            return reply(
                StatusCode::GATEWAY_TIMEOUT,
                "timeout",
                "no agent reply in time",
            );
        }
        tokio::time::sleep(poll).await;
    };
    let text = answer.body.clone().unwrap_or_default();
    if pc.post_reply(hb.task_id, &text).await.is_err() {
        return reply(
            StatusCode::BAD_GATEWAY,
            "error",
            "paperclip reply post failed",
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "runId": hb.run_id,
            "result": text,
            "denAgentId": den,
            "messageId": answer.id,
            "latencyMs": started.elapsed().as_millis() as u64,
        })),
    )
}

pub(super) async fn heartbeat(
    State(store): State<LocalHubStore>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    handle(store, &seats_path(), POLL, &headers, &body).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::Path, routing::get, Router};
    use std::sync::{Arc, Mutex};

    const SECRET: &str = "s3cret-token";

    fn hdr(secret: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(s) = secret {
            h.insert("authorization", format!("Bearer {s}").parse().unwrap());
        }
        h
    }

    #[test]
    fn constant_time_compare() {
        assert!(ct_eq("abc", "abc"));
        assert!(!ct_eq("abc", "abd"));
        assert!(!ct_eq("abc", "abcd"));
        let seat = Seat {
            den_agent_id: Uuid::nil(),
            conversation_id: Uuid::nil(),
            secret_sha256: digest(SECRET).to_ascii_uppercase(),
            paperclip_base_url: None,
        };
        assert!(secret_matches(&seat, SECRET));
        assert!(!secret_matches(&seat, "other"));
    }

    #[test]
    fn payload_validation() {
        let t = Uuid::new_v4();
        let good = json!({"runId":"r1","agentId":"a","companyId":Uuid::new_v4(),
            "context":{"taskId":t,"wakeReason":"assigned"},"timeoutSec":9999});
        let h = parse_heartbeat(&good).unwrap();
        assert_eq!(h.task_id, t);
        assert_eq!(h.timeout, Duration::from_secs(MAX_TIMEOUT_SECS));
        assert!(h.comment_id.is_none());
        // The shape Paperclip's http adapter really sends: no companyId, no timeoutSec.
        let real = json!({"agentId":"a","runId":"r2","context":{"taskId":t,"issueId":t,"wakeReason":"issue_assigned"}});
        assert_eq!(parse_heartbeat(&real).unwrap().task_id, t);
        for bad in [
            json!({"agentId":"a","companyId":Uuid::new_v4(),"context":{"taskId":t}}),
            json!({"runId":"r","companyId":"nope","context":{"taskId":t}}),
            json!({"runId":"r","companyId":Uuid::new_v4(),"context":{"taskId":"../x"}}),
            json!({"runId":"r","companyId":Uuid::new_v4(),"context":{"taskId":t,"commentId":"x"}}),
            json!({"runId":"r","companyId":Uuid::new_v4()}),
        ] {
            assert!(parse_heartbeat(&bad).is_err(), "{bad}");
        }
        assert_eq!(clip(&"é".repeat(MAX_TICKET_TEXT)).len(), MAX_TICKET_TEXT);
    }

    struct Rig {
        store: LocalHubStore,
        den: Uuid,
        conv: Uuid,
        rev: u32,
        owner: Uuid,
        seats: PathBuf,
        posted: Arc<Mutex<Vec<Value>>>,
        task: Uuid,
    }

    async fn mock_paperclip(task: Uuid, posted: Arc<Mutex<Vec<Value>>>) -> String {
        let issue = move |Path(id): Path<String>| async move {
            assert_eq!(id, task.to_string());
            Json(json!({"title":"Fix login","description":"It is broken"}))
        };
        // Our reply is a PATCH on the ticket itself (status + comment), not a POST to
        // /comments -- posting a plain comment is what caused the wake loop this fixes.
        let issue_patch = move |Path(id): Path<String>, Json(b): Json<Value>| {
            let posted = posted.clone();
            async move {
                assert_eq!(id, task.to_string());
                posted.lock().unwrap().push(b);
                Json(json!({"id": task}))
            }
        };
        let comments = get(|| async { Json(json!([])) });
        let app = Router::new()
            .route("/api/issues/:id", get(issue).patch(issue_patch))
            .route("/api/issues/:id/comments", comments);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}")
    }

    async fn rig() -> Rig {
        let store = LocalHubStore::in_memory().unwrap();
        let owner = Uuid::new_v4();
        let agent = store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Den".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(Uuid::new_v4()),
                capability_policy_ref: "default".into(),
                provider_account_ref: None,
                memory_namespace: "pc".into(),
            })
            .unwrap();
        let c = store
            .bots_conversations_create(NewConversation {
                title: None,
                owner,
                kind: ConversationKind::AgentDm,
                project_id: None,
                coordinator: Some(agent.id),
                storage_scope: StorageScope::LocalOnly,
            })
            .unwrap();
        let task = Uuid::new_v4();
        let posted = Arc::new(Mutex::new(vec![]));
        let url = mock_paperclip(task, posted.clone()).await;
        let seats = std::env::temp_dir().join(format!("pc-seats-{}.json", Uuid::new_v4()));
        std::fs::write(
            &seats,
            json!({"pc-agent": {"den_agent_id": agent.id, "conversation_id": c.id,
                "secret_sha256": digest(SECRET), "paperclip_base_url": url}})
            .to_string(),
        )
        .unwrap();
        Rig {
            store,
            den: agent.id,
            conv: c.id,
            rev: c.policy_revision,
            owner,
            seats,
            posted,
            task,
        }
    }

    fn body(r: &Rig, run: &str, agent: &str, timeout: u64) -> Vec<u8> {
        json!({"runId":run,"agentId":agent,"companyId":Uuid::new_v4(),
            "context":{"taskId":r.task,"wakeReason":"assigned"},"timeoutSec":timeout})
        .to_string()
        .into_bytes()
    }

    async fn call(r: &Rig, secret: Option<&str>, b: &[u8]) -> (StatusCode, Value) {
        let poll = Duration::from_millis(20);
        let (c, Json(v)) = handle(r.store.clone(), &r.seats, poll, &hdr(secret), b).await;
        (c, v)
    }

    fn all_messages(r: &Rig) -> Vec<Message> {
        let page = MessagePage {
            before: None,
            after: None,
            limit: 50,
        };
        r.store
            .bots_messages_list(Principal::User(r.owner), r.conv, page)
            .unwrap()
    }

    #[tokio::test]
    async fn auth_failures_are_401_without_detail() {
        let r = rig().await;
        let ok_body = body(&r, "r1", "pc-agent", 1);
        for (secret, b) in [
            (None, ok_body.clone()),
            (Some("wrong"), ok_body.clone()),
            (Some(SECRET), body(&r, "r1", "unknown-agent", 1)),
            (Some(SECRET), br#"{"runId":"x"}"#.to_vec()),
        ] {
            let (code, v) = call(&r, secret, &b).await;
            assert_eq!(code, StatusCode::UNAUTHORIZED);
            assert_eq!(v, json!({"status":"unauthorized","reason":""}));
        }
        // Missing seats file also 401.
        let missing = FsPath::new("/nonexistent/seats.json");
        let (c, _) = handle(r.store.clone(), missing, POLL, &hdr(Some(SECRET)), &ok_body).await;
        assert_eq!(c, StatusCode::UNAUTHORIZED);
        assert!(
            all_messages(&r).is_empty(),
            "nothing may be sent unauthenticated"
        );
        let big = vec![b'x'; MAX_BODY + 1];
        assert_eq!(
            call(&r, Some(SECRET), &big).await.0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let mut bad = serde_json::from_slice::<Value>(&ok_body).unwrap();
        bad["context"]["taskId"] = json!("nope");
        let (code, _) = call(&r, Some(SECRET), bad.to_string().as_bytes()).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn round_trip_idempotent_and_timeout() {
        let r = rig().await;
        // Simulated agent host: answers in the conversation after a short delay.
        let (s, den, conv, rev) = (r.store.clone(), r.den, r.conv, r.rev);
        let agent = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            s.bots_message_send(
                Principal::Agent(den),
                conv,
                "agent-reply-1".into(),
                rev,
                vec![],
                NewMessage {
                    thread_root: None,
                    kind: MessageKind::Text,
                    body: Some("done: fixed".into()),
                    attachment_refs: vec![],
                    task_ref: None,
                    turn_ref: None,
                    source_event_ref: None,
                },
            )
            .unwrap()
        });
        let b = body(&r, "run-1", "pc-agent", 10);
        let (code, v) = call(&r, Some(SECRET), &b).await;
        let reply_msg = agent.await.unwrap();
        assert_eq!(code, StatusCode::OK, "{v}");
        assert_eq!(v["status"], "ok");
        assert_eq!(v["result"], "done: fixed");
        assert_eq!(v["messageId"], json!(reply_msg.id));
        assert_eq!(v["denAgentId"], json!(r.den));
        let first_post = vec![json!({"status":"in_review","comment":"done: fixed"})];
        assert_eq!(r.posted.lock().unwrap().clone(), first_post);

        // Same runId again: no second owner message, existing reply returned.
        let (code, v) = call(&r, Some(SECRET), &b).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(v["result"], "done: fixed");
        let all = all_messages(&r);
        let from_owner: Vec<_> = all
            .iter()
            .filter(|m| m.author == Principal::User(r.owner))
            .collect();
        assert_eq!(from_owner.len(), 1);
        assert_eq!(from_owner[0].client_request_id, "paperclip:run-1");
        assert!(from_owner[0].body.as_deref().unwrap().contains("Fix login"));

        // New run, agent never answers: 504 and nothing more posted to Paperclip.
        let (code, v) = call(&r, Some(SECRET), &body(&r, "run-2", "pc-agent", 1)).await;
        assert_eq!(code, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(v["status"], "timeout");
        assert_eq!(
            r.posted.lock().unwrap().len(),
            2,
            "only the retry's comment"
        );
    }

    #[tokio::test]
    async fn failed_delivery_returns_502_fast() {
        let r = rig().await;
        // Simulated runner: claims the delivery then reports failure, with no reply message.
        let (s, den, conv, owner) = (r.store.clone(), r.den, r.conv, r.owner);
        let runner = tokio::spawn(async move {
            let page = MessagePage {
                before: None,
                after: None,
                limit: 10,
            };
            let msg = loop {
                let ms = s
                    .bots_messages_list(Principal::User(owner), conv, page)
                    .unwrap();
                if let Some(m) = ms.into_iter().next() {
                    break m;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            };
            let key = DeliveryKey {
                message_id: msg.id,
                recipient: den,
            };
            let d = s.bots_delivery_claim(key).unwrap();
            s.bots_delivery_fail(key, d.lease_generation, None).unwrap();
        });
        let t = Instant::now();
        let (code, v) = call(&r, Some(SECRET), &body(&r, "run-f", "pc-agent", 30)).await;
        runner.await.unwrap();
        assert_eq!(code, StatusCode::BAD_GATEWAY, "{v}");
        assert_eq!(v["status"], "delivery_failed");
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "must not wait out the timeout"
        );
        assert!(
            r.posted.lock().unwrap().is_empty(),
            "nothing posted to Paperclip"
        );
    }

    #[tokio::test]
    async fn paperclip_down_is_502() {
        let r = rig().await;
        std::fs::write(
            &r.seats,
            json!({"pc-agent": {"den_agent_id": r.den, "conversation_id": r.conv,
                "secret_sha256": digest(SECRET), "paperclip_base_url": "http://127.0.0.1:1"}})
            .to_string(),
        )
        .unwrap();
        let (code, _) = call(&r, Some(SECRET), &body(&r, "r", "pc-agent", 1)).await;
        assert_eq!(code, StatusCode::BAD_GATEWAY);
    }
}
