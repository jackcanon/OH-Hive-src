//! Project-scoped live tickets only; never accepts or stores a member account JWT.
//! Boards include viewer-specific fields. Poll per subscriber rather than replaying another
//! member's cached board. Bound subscriptions, deadlines and buffers; fail closed on errors.
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::IntoResponse,
};
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
const MAX_SUBSCRIBERS: usize = 64;
const POLL: Duration = Duration::from_secs(2);
#[derive(Clone)]
pub struct Live {
    inner: Arc<Inner>,
}
struct Inner {
    http: reqwest::Client,
    endpoint: String,
    anon: String,
    node_key: String,
    slots: Arc<Semaphore>,
}
#[derive(Deserialize)]
pub struct LiveQuery {
    token: String,
}
#[derive(Deserialize)]
struct Board {
    board: serde_json::Value,
    expires_at: u64,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
impl Live {
    pub fn new(hub_url: &str, anon: &str, node_key: &str) -> Self {
        Self {
            inner: Arc::new(Inner {
                http: reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(5))
                    .timeout(Duration::from_secs(10))
                    .build()
                    .expect("live HTTP client"),
                endpoint: format!(
                    "{}/rest/v1/rpc/hive_project_board_for",
                    hub_url.trim_end_matches('/')
                ),
                anon: anon.into(),
                node_key: node_key.into(),
                slots: Arc::new(Semaphore::new(MAX_SUBSCRIBERS)),
            }),
        }
    }
    pub async fn subscribers(&self) -> usize {
        MAX_SUBSCRIBERS - self.inner.slots.available_permits()
    }
    async fn board(&self, project: &str, token: &str) -> Result<Board, ()> {
        let mut response = self.inner.http.post(&self.inner.endpoint).header("apikey",&self.inner.anon).bearer_auth(&self.inner.anon)
            .json(&serde_json::json!({"raw_key":self.inner.node_key,"p_project_id":project,"p_token":token})).send().await.map_err(|_| ())?;
        if !response.status().is_success() {
            return Err(());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
            if bytes.len().saturating_add(chunk.len()) > 8 * 1024 * 1024 {
                return Err(());
            }
            bytes.extend_from_slice(&chunk);
        }
        let result: Board = serde_json::from_slice(&bytes).map_err(|_| ())?;
        if result.expires_at <= now()
            || result.expires_at > now() + 300
            || result
                .board
                .get("project")
                .and_then(|p| p.get("id"))
                .and_then(|id| id.as_str())
                != Some(project)
        {
            return Err(());
        }
        Ok(result)
    }
}
fn valid_input(project: &str, token: &str) -> bool {
    uuid::Uuid::parse_str(project).is_ok()
        && token.starts_with("hive_live_v1.")
        && token.len() <= 512
        && token.split('.').count() == 7
}
pub async fn live_ws(
    State(live): State<Live>,
    Path(project): Path<String>,
    Query(q): Query<LiveQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    if !valid_input(&project, &q.token) {
        return (StatusCode::BAD_REQUEST, "project ticket required").into_response();
    }
    let permit = match live.inner.slots.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return (StatusCode::TOO_MANY_REQUESTS, "live capacity reached").into_response(),
    };
    // Authenticate before upgrade, before replay, and before allocating a polling task.
    let initial = match live.board(&project, &q.token).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::UNAUTHORIZED, "live access unavailable").into_response(),
    };
    ws.max_message_size(4096)
        .max_frame_size(4096)
        .on_upgrade(move |socket| handle(socket, live, project, q.token, initial, permit))
}
async fn send(socket: &mut WebSocket, message: Message) -> Result<(), ()> {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}
async fn handle(
    mut socket: WebSocket,
    live: Live,
    project: String,
    token: String,
    initial: Board,
    _permit: OwnedSemaphorePermit,
) {
    let expires =
        tokio::time::Instant::now() + Duration::from_secs(initial.expires_at.saturating_sub(now()));
    let mut previous = None;
    let mut next = Some(initial);
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + POLL, POLL);
    loop {
        if let Some(board) = next.take() {
            if now() >= board.expires_at {
                break;
            }
            let frame =
                serde_json::json!({"type":"board","project_id":project,"board":board.board})
                    .to_string();
            if previous.as_ref() != Some(&frame) {
                if send(&mut socket, Message::Text(frame.clone()))
                    .await
                    .is_err()
                {
                    break;
                }
                previous = Some(frame);
            }
        }
        tokio::select! {
            _=tokio::time::sleep_until(expires)=>break,
            _=tick.tick()=>{
                // Any denial/outage stops this subscription. Browser falls back to hub polling.
                let result=tokio::time::timeout_at(expires,live.board(&project,&token)).await;
                match result { Ok(Ok(board))=>next=Some(board),_=>break }
            },
            msg=socket.recv()=>match msg {
                Some(Ok(Message::Ping(p)))=>{if send(&mut socket,Message::Pong(p)).await.is_err(){break;}},
                Some(Ok(Message::Close(_)))|None|Some(Err(_))=>break,
                _=>{}
            }
        }
    }
    let _ = send(&mut socket, Message::Close(None)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        routing::{get, post},
        Json, Router,
    };
    use futures::StreamExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    const PROJECT: &str = "33333333-3333-3333-3333-333333333333";
    // Tokens here are opaque fixtures; cryptographic verification is covered by PostgreSQL tests.
    fn ticket(viewer: &str) -> String {
        format!("hive_live_v1.{viewer}.project.server.exp.nonce.sig")
    }
    async fn fixture() -> (
        Live,
        String,
        Arc<AtomicBool>,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let revoked = Arc::new(AtomicBool::new(false));
        let flag = revoked.clone();
        let app=Router::new().route("/rest/v1/rpc/hive_project_board_for",post(move |headers:axum::http::HeaderMap,Json(body):Json<serde_json::Value>|{
            let flag=flag.clone(); async move {
                assert_eq!(headers.get("authorization").unwrap(),"Bearer anon");
                assert_eq!(body["raw_key"],"node-key");
                let token=body["p_token"].as_str().unwrap();
                if flag.load(Ordering::Relaxed)||token.contains("forged") { return (axum::http::StatusCode::UNAUTHORIZED,Json(serde_json::json!({"error":"private error must not escape"}))); }
                (axum::http::StatusCode::OK,Json(serde_json::json!({"board":{"project":{"id":body["p_project_id"],"my_role":token}},"expires_at":now()+if token.contains("short") {1} else {300}})))
            }
        }));
        let hub = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let hub_url = format!("http://{}", hub.local_addr().unwrap());
        let hub_task = tokio::spawn(async move {
            axum::serve(hub, app).await.unwrap();
        });
        let live = Live::new(&hub_url, "anon", "node-key");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/live/{PROJECT}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/live/:project_id", get(live_ws))
            .with_state(live.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (live, url, revoked, hub_task, server)
    }
    #[tokio::test]
    async fn invalid_subscriber_cannot_join_or_poison_another_viewer() {
        let (live, url, revoked, hub, server) = fixture().await;
        let (mut alice, _) =
            tokio_tungstenite::connect_async(format!("{url}?token={}", ticket("alice")))
                .await
                .unwrap();
        let first = alice.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(first.contains("alice"));
        assert!(
            tokio_tungstenite::connect_async(format!("{url}?token={}", ticket("forged")))
                .await
                .is_err()
        );
        assert_eq!(live.subscribers().await, 1);
        let (mut bob, _) =
            tokio_tungstenite::connect_async(format!("{url}?token={}", ticket("bob")))
                .await
                .unwrap();
        let second = bob.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(second.contains("bob") && !second.contains("alice"));
        revoked.store(true, Ordering::Relaxed);
        let closed = tokio::time::timeout(Duration::from_secs(4), alice.next())
            .await
            .unwrap();
        assert!(
            closed.is_none()
                || matches!(
                    closed,
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_)))
                )
        );
        let _ = bob.close(None).await;
        hub.abort();
        server.abort();
    }
    #[tokio::test]
    async fn capacity_expiry_and_member_jwt_rejection() {
        let (live, url, _, hub, server) = fixture().await;
        assert!(
            tokio_tungstenite::connect_async(format!("{url}?token=eyJ.member.fullJwt"))
                .await
                .is_err()
        );
        let all = live
            .inner
            .slots
            .clone()
            .acquire_many_owned(MAX_SUBSCRIBERS as u32)
            .await
            .unwrap();
        match tokio_tungstenite::connect_async(format!("{url}?token={}", ticket("alice"))).await {
            Err(tokio_tungstenite::tungstenite::Error::Http(r)) => assert_eq!(r.status(), 429),
            _ => panic!("expected capacity refusal"),
        }
        drop(all);
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("{url}?token={}", ticket("short")))
                .await
                .unwrap();
        let _ = socket.next().await;
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(3), socket.next())
                .await
                .unwrap(),
            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_)))
        ));
        hub.abort();
        server.abort();
    }
}
