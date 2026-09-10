//! Live broadcast (ADR-013 §A.4): `GET /live/<project_id>?token=<member jwt>` → WebSocket.
//!
//! One poller per project, not per browser: the first subscriber's JWT is used to read
//! `project_board` through the hub every `POLL` seconds (RLS applies — read-all per D8, so any
//! member's view of a board is the same), the JSON is hashed, and a frame goes out only when it
//! changed. Subscribers get the current board immediately on connect. When the last subscriber
//! leaves, the poller stops. This turns N browsers polling Postgres into 1 poll per project;
//! when the coordinator owns placement it will feed this from memory instead of polling.
//!
//! Frames: `{"type":"board","project_id":…,"board":{…},"at":…}` and `{"type":"error",…}`.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::IntoResponse,
};
use hive_core::hub::MemberClient;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{broadcast, Mutex};

const POLL: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct Live {
    member: Arc<MemberClient>,
    rooms: Arc<Mutex<HashMap<String, Room>>>,
}

struct Room {
    tx: broadcast::Sender<String>,
    subscribers: usize,
    /// latest JWT seen from a subscriber; the poller reads with it (refreshes as browsers reconnect)
    jwt: Arc<Mutex<String>>,
    last: Arc<Mutex<Option<String>>>, // last frame, replayed to new subscribers
    stop: Arc<tokio::sync::Notify>,
}

#[derive(Deserialize)]
pub struct LiveQuery {
    token: String,
}

impl Live {
    pub fn new(member: MemberClient) -> Self {
        Self {
            member: Arc::new(member),
            rooms: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn subscribers(&self) -> usize {
        self.rooms
            .lock()
            .await
            .values()
            .map(|r| r.subscribers)
            .sum()
    }

    async fn join(
        &self,
        project_id: &str,
        jwt: String,
    ) -> (broadcast::Receiver<String>, Option<String>) {
        let mut rooms = self.rooms.lock().await;
        if let Some(room) = rooms.get_mut(project_id) {
            room.subscribers += 1;
            *room.jwt.lock().await = jwt;
            let last = room.last.lock().await.clone();
            return (room.tx.subscribe(), last);
        }
        let (tx, rx) = broadcast::channel(16);
        let room = Room {
            tx: tx.clone(),
            subscribers: 1,
            jwt: Arc::new(Mutex::new(jwt)),
            last: Arc::new(Mutex::new(None)),
            stop: Arc::new(tokio::sync::Notify::new()),
        };
        let (member, pid, jwt_ref, last_ref, stop) = (
            self.member.clone(),
            project_id.to_string(),
            room.jwt.clone(),
            room.last.clone(),
            room.stop.clone(),
        );
        tokio::spawn(async move {
            let mut prev_hash: Option<String> = None;
            let mut tick = tokio::time::interval(POLL);
            loop {
                tokio::select! {
                    _ = stop.notified() => break,
                    _ = tick.tick() => {}
                }
                let jwt = jwt_ref.lock().await.clone();
                let frame = match member.rpc(&jwt, "hive_project_board", serde_json::json!({ "p_project_id": pid })).await {
                    Ok(board) => {
                        let body = board.to_string();
                        let h = hex::encode(Sha256::digest(body.as_bytes()));
                        if prev_hash.as_deref() == Some(&h) {
                            continue;
                        }
                        prev_hash = Some(h);
                        serde_json::json!({ "type": "board", "project_id": pid, "board": board, "at": chrono_now() }).to_string()
                    }
                    Err(e) => serde_json::json!({ "type": "error", "project_id": pid, "error": e.to_string() }).to_string(),
                };
                *last_ref.lock().await = Some(frame.clone());
                let _ = tx.send(frame);
            }
            tracing::debug!(project = %pid, "live poller stopped");
        });
        rooms.insert(project_id.to_string(), room);
        (rx, None)
    }

    async fn leave(&self, project_id: &str) {
        let mut rooms = self.rooms.lock().await;
        if let Some(room) = rooms.get_mut(project_id) {
            room.subscribers = room.subscribers.saturating_sub(1);
            if room.subscribers == 0 {
                room.stop.notify_one();
                rooms.remove(project_id);
            }
        }
    }
}

fn chrono_now() -> String {
    // avoid pulling chrono into the server just for this
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", d.as_secs())
}

pub async fn live_ws(
    State(live): State<Live>,
    Path(project_id): Path<String>,
    Query(q): Query<LiveQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if uuid::Uuid::parse_str(&project_id).is_err() || q.token.len() < 20 {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "project id + token required",
        )
            .into_response();
    }
    ws.on_upgrade(move |socket| handle(socket, live, project_id, q.token))
}

async fn handle(mut socket: WebSocket, live: Live, project_id: String, jwt: String) {
    let (mut rx, last) = live.join(&project_id, jwt).await;
    if let Some(frame) = last {
        if socket.send(Message::Text(frame)).await.is_err() {
            live.leave(&project_id).await;
            return;
        }
    }
    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Ok(frame) => { if socket.send(Message::Text(frame)).await.is_err() { break; } }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(Message::Ping(p))) => { let _ = socket.send(Message::Pong(p)).await; }
                _ => {}
            }
        }
    }
    live.leave(&project_id).await;
}
