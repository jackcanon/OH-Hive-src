//! `hive-server` — regional server (ADR-004 / ADR-007 / ADR-013 §F), v0.
//!
//! v0 does the part that unblocks binary outputs today:
//! - registers with the hub as a regional server (operator/tier per ADR-013 D76), heartbeats,
//!   competes for the coordinator lease
//! - content-addressed artifact store on local disk: `PUT /a` (uploader = any paired node,
//!   verified against the hub), `GET /a/<sha256>`, `HEAD /a/<sha256>`
//! - announces every blob it holds to the hub (hive.artifact_replicas) so `artifact_locate`
//!   can hand members a URL
//!
//! - live board broadcast (`/live/<project_id>`), read-all snapshot (`/snapshot/latest`),
//!   coordinator election, pull-based replication to factor 2
//!
//! - nightly encrypted hub backups on HJM-operated coordinators (`backup.rs`, ADR-013 D73)
//! - ledger archival past the 90-day hot window, one month at a time (`ledger_archive.rs`, D73)
//! - garbage collection of unpinned blobs after grace (`replicate::gc_tick`)
//!
//! Not yet: libp2p relay for nodes behind NAT, model-weight cache. Same binary.
//!
//! Footprint rule: single static executable, no Python, no GPU deps, Pi-class RAM.
//! Pairing: `hive pair` (choose "Regional server" on the web page) writes the same node.env this
//! binary reads — one identity mechanism for both shells.

pub mod backup;
pub mod ledger_archive;
pub mod live;
pub mod replicate;
pub mod snapshot;
pub mod store;

use anyhow::{Context, Result};
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, put},
    Json, Router,
};
use hive_core::hub::{ArtifactAnnounce, HubClient, MemberClient, ServerRegistration};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone)]
struct App {
    hub: Arc<HubClient>,
    store: Arc<store::Store>,
    connections: Arc<AtomicU32>,
    /// true while this server holds hive.coordinator_lease
    is_coordinator: Arc<AtomicBool>,
    /// live board broadcast rooms (ADR-013 §A.4)
    live: live::Live,
    /// read-all snapshot (ADR-013 §A.5)
    snapshot: snapshot::Snapshot,
    node_key: String,
    /// public_url of the current coordinator (from the lease reply), for snapshot relay
    coordinator_url: Arc<Mutex<Option<String>>>,
    /// uploader node key → (node id, verified at). Keys are verified against the hub, cached 5 min.
    verified: Arc<Mutex<HashMap<String, (uuid::Uuid, Instant)>>>,
}

impl axum::extract::FromRef<App> for live::Live {
    fn from_ref(app: &App) -> live::Live {
        app.live.clone()
    }
}

/// Everything `hive-server serve` takes. Mirrors the `HIVE_*` env keys written by `hive set`.
#[derive(Clone, Debug)]
pub struct ServeOptions {
    pub public_url: String,
    pub listen: String,
    pub data_dir: Option<PathBuf>,
    pub storage_gb: u32,
    pub operator: String,
    pub tier: String,
    pub region: Option<String>,
    pub max_upload_mb: usize,
    pub backup_recipient: Option<String>,
    pub backup_hour_utc: u32,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            public_url: String::new(),
            listen: "0.0.0.0:8790".into(),
            data_dir: None,
            storage_gb: 50,
            operator: "volunteer".into(),
            tier: "primary".into(),
            region: None,
            max_upload_mb: 512,
            backup_recipient: None,
            backup_hour_utc: 9,
        }
    }
}

/// Live counters a shell (CLI log line, desktop pane) can read while `serve` runs.
#[derive(Default)]
pub struct ServerStatus {
    pub registered: AtomicBool,
    pub coordinator: AtomicBool,
    pub coordinator_name: Mutex<Option<String>>,
    pub blobs: AtomicU64,
    pub used_bytes: AtomicU64,
    pub last_backup: Mutex<Option<String>>,
}

/// Register, serve, heartbeat, replicate, gc, back up — until `stop` resolves; then step down and
/// check out. This is the whole regional server; `main.rs` and the desktop app both call it.
pub async fn serve(
    cfg: &hive_core::nodeconfig::NodeConfig,
    hub: Arc<HubClient>,
    opts: ServeOptions,
    stop: impl std::future::Future<Output = ()> + Send + 'static,
    status: Arc<ServerStatus>,
) -> Result<()> {
    let ServeOptions {
        public_url,
        listen,
        data_dir,
        storage_gb,
        operator,
        tier,
        region,
        max_upload_mb,
        backup_recipient,
        backup_hour_utc,
    } = opts;

    let data_dir = data_dir.unwrap_or_else(default_data_dir);
    let backup = backup::Backup::from_env(&data_dir, backup_recipient.as_deref(), backup_hour_utc)?;
    if backup.is_some() && operator != "hjm" {
        tracing::warn!(
            "HIVE_BACKUP_RECIPIENT set but operator is not hjm — backups will not run here"
        );
    }
    // Ledger archival reuses the same hub recipient key as backups — one "hub key" to manage,
    // not two (ADR-013 D73).
    let archiver = ledger_archive::LedgerArchiver::from_env(backup_recipient.as_deref())?;
    let st = Arc::new(store::Store::open(&data_dir)?);
    let is_hjm = operator == "hjm";
    let reg = ServerRegistration {
        public_url: public_url.trim_end_matches('/').to_string(),
        multiaddrs: vec![],
        operator,
        tier,
        storage_gb: Some(storage_gb),
        region: region.or(cfg.region.clone()),
    };
    let r = hub
        .server_register(&reg)
        .await
        .context("register with hub")?;
    tracing::info!(reply = %r, "registered as regional server");
    status.registered.store(true, Ordering::Relaxed);

    // Re-announce what we already hold (a restarted server must not forget its blobs).
    let held = st.list()?;
    for (hash, bytes) in &held {
        let _ = hub
            .artifact_announce(&ArtifactAnnounce {
                hash: hash.clone(),
                bytes: *bytes,
                mime: "application/octet-stream".into(),
                kind: "output".into(),
                ..Default::default()
            })
            .await;
    }
    tracing::info!(blobs = held.len(), dir = %data_dir.display(), "artifact store ready");

    let app = App {
        hub: hub.clone(),
        store: st.clone(),
        connections: Arc::new(AtomicU32::new(0)),
        is_coordinator: Arc::new(AtomicBool::new(false)),
        live: live::Live::new(MemberClient::new(&cfg.hub_url, &cfg.anon_key)),
        snapshot: snapshot::Snapshot::new(MemberClient::new(&cfg.hub_url, &cfg.anon_key)),
        node_key: cfg.node_key.clone().unwrap_or_default(),
        coordinator_url: Arc::new(Mutex::new(None)),
        verified: Arc::new(Mutex::new(HashMap::new())),
    };
    let router = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/a", put(put_blob))
        .route("/a/:hash", get(get_blob).head(head_blob))
        .route("/live/:project_id", get(live::live_ws))
        .route("/snapshot/latest", get(snapshot_latest))
        // axum's own 2 MB default limit runs before tower-http's; raise both to --max-upload-mb
        .layer(axum::extract::DefaultBodyLimit::max(
            max_upload_mb * 1024 * 1024,
        ))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            max_upload_mb * 1024 * 1024,
        ))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(app.clone());

    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .with_context(|| format!("bind {listen}"))?;
    tracing::info!(%listen, public_url = %reg.public_url, "serving");

    // heartbeat + coordinator election every 30 s (lease TTL 90 s → failover ≤ 2 min, ADR-005)
    let hb = {
        let app = app.clone();
        let status = status.clone();
        tokio::spawn(async move {
            let mut t = tokio::time::interval(Duration::from_secs(30));
            let mut was_coordinator = false;
            // This server's hub round-trip time, as measured on the previous heartbeat -- fed
            // back into the next call so the hub always has a (one-interval-stale) number.
            let mut last_rtt_ms: Option<u64> = None;
            loop {
                t.tick().await;
                let used = app.store.used_bytes().unwrap_or(0);
                status.used_bytes.store(used, Ordering::Relaxed);
                status
                    .blobs
                    .store(app.store.count().unwrap_or(0) as u64, Ordering::Relaxed);
                match app
                    .hub
                    .server_heartbeat(used, app.connections.load(Ordering::Relaxed), last_rtt_ms)
                    .await
                {
                    Ok((_, rtt)) => last_rtt_ms = Some(rtt),
                    Err(e) => tracing::warn!("heartbeat failed: {e}"),
                }
                match app.hub.coordinator_try(90).await {
                    Ok(l) => {
                        app.is_coordinator.store(l.coordinator, Ordering::Relaxed);
                        status.coordinator.store(l.coordinator, Ordering::Relaxed);
                        *status.coordinator_name.lock().await = l.holder_name.clone();
                        if l.coordinator && !was_coordinator {
                            tracing::info!(
                                generation = l.generation,
                                "★ this server is now the Hive coordinator"
                            );
                        } else if !l.coordinator && was_coordinator {
                            tracing::warn!(holder = ?l.holder_name, "lost the coordinator lease");
                        }
                        was_coordinator = l.coordinator;
                        *app.coordinator_url.lock().await = l.holder_url.clone();
                    }
                    Err(e) => tracing::warn!("coordinator election call failed: {e}"),
                }
            }
        })
    };

    // snapshot: coordinator pulls from the hub, others relay the coordinator's copy (ADR-013 §A.5)
    let snap = {
        let app = app.clone();
        tokio::spawn(async move {
            let mut t = tokio::time::interval(snapshot::EVERY);
            loop {
                t.tick().await;
                let url = app.coordinator_url.lock().await.clone();
                app.snapshot
                    .refresh(
                        &app.hub,
                        app.is_coordinator.load(Ordering::Relaxed),
                        url.as_deref(),
                        &app.node_key,
                    )
                    .await;
            }
        })
    };

    // replication: pull blobs below replication factor from other servers (ADR-007)
    let repl = {
        let app = app.clone();
        tokio::spawn(async move {
            let http = reqwest::Client::new();
            let mut t = tokio::time::interval(replicate::EVERY);
            loop {
                t.tick().await;
                let n = replicate::tick(&app.hub, &app.store, &http).await;
                if n > 0 {
                    tracing::info!(replicated = n, "replication pass");
                }
            }
        })
    };

    // garbage collection: drop blobs the hub says are unpinned past grace (every 6 h, first pass after 10 min)
    let gc = {
        let app = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(600)).await;
            let mut t = tokio::time::interval(replicate::GC_EVERY);
            loop {
                t.tick().await;
                let n = replicate::gc_tick(&app.hub, &app.store).await;
                if n > 0 {
                    tracing::info!(dropped = n, "gc pass");
                }
            }
        })
    };

    // nightly hub backup: HJM-operated coordinator only (ADR-013 D73)
    let bk = {
        let app = app.clone();
        let status = status.clone();
        let backup = backup.map(Arc::new);
        tokio::spawn(async move {
            let Some(b) = backup else { return };
            tracing::info!(hour_utc = b_hour(&b), "nightly hub backups enabled");
            let mut t = tokio::time::interval(backup::CHECK_EVERY);
            loop {
                t.tick().await;
                if !is_hjm || !app.is_coordinator.load(Ordering::Relaxed) || !b.due() {
                    continue;
                }
                match b.run(&app.hub, &app.store).await {
                    Ok((hash, bytes, plain)) => {
                        tracing::info!(hash = %&hash[..12], bytes, plain, "★ hub backup stored + announced");
                        *status.last_backup.lock().await = Some(hash);
                    }
                    Err(e) => tracing::error!("hub backup failed: {e:#}"),
                }
            }
        })
    };

    // ledger archival past the 90-day hot window: same HJM-operated-coordinator gate as backups,
    // one month per tick so a long backlog drains gradually instead of blocking the whole tick.
    let la = {
        let app = app.clone();
        let archiver = archiver.map(Arc::new);
        tokio::spawn(async move {
            let Some(a) = archiver else { return };
            tracing::info!("ledger archival enabled (90-day hot window, ADR-013 D73)");
            let mut t = tokio::time::interval(ledger_archive::CHECK_EVERY);
            loop {
                t.tick().await;
                if !is_hjm || !app.is_coordinator.load(Ordering::Relaxed) {
                    continue;
                }
                match a.run_one(&app.hub, &app.store).await {
                    Ok(Some((month, hash, bytes, entries))) => {
                        tracing::info!(month, hash = %&hash[..12], bytes, entries, "★ ledger month archived");
                    }
                    Ok(None) => {}
                    Err(e) => tracing::error!("ledger archival failed: {e:#}"),
                }
            }
        })
    };

    axum::serve(listener, router)
        .with_graceful_shutdown(stop)
        .await?;
    hb.abort();
    snap.abort();
    repl.abort();
    gc.abort();
    bk.abort();
    la.abort();
    if app.is_coordinator.load(Ordering::Relaxed) {
        let _ = hub.coordinator_release().await;
        tracing::info!("stepped down as coordinator");
    }
    let _ = hub.check_out().await;
    tracing::info!("checked out; bye");
    Ok(())
}

/// One backup now (export → gzip → age → store → announce). Needs operator=hjm on the hub.
pub async fn backup_once(
    hub: &HubClient,
    data_dir: Option<PathBuf>,
    recipient: &str,
) -> Result<(String, u64, usize)> {
    let data_dir = data_dir.unwrap_or_else(default_data_dir);
    let st = Arc::new(store::Store::open(&data_dir)?);
    let b =
        backup::Backup::from_env(&data_dir, Some(recipient), 0)?.context("recipient required")?;
    b.run(hub, &st).await
}

/// One garbage-collection pass now. Returns (dropped, blobs left).
pub async fn gc_once(hub: &HubClient, data_dir: Option<PathBuf>) -> Result<(usize, usize)> {
    let st = Arc::new(store::Store::open(
        &data_dir.unwrap_or_else(default_data_dir),
    )?);
    let n = replicate::gc_tick(hub, &st).await;
    Ok((n, st.count().unwrap_or(0)))
}

/// `GET /snapshot/latest?token=<member jwt>` or `Authorization: Bearer <node key>`.
async fn snapshot_latest(
    State(app): State<App>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<snapshot::SnapQuery>,
) -> impl IntoResponse {
    let node_ok = match snapshot::Snapshot::has_node_key(&headers) {
        Some(k) if q.token.is_none() => verify_node_key(&app, &k).await.is_some(),
        _ => false,
    };
    app.snapshot.respond(&headers, &q, node_ok).await
}

async fn root() -> impl IntoResponse {
    Json(
        serde_json::json!({ "service": "hive-server", "version": hive_core::VERSION, "endpoints": ["/health", "PUT /a", "GET /a/<sha256>", "WS /live/<project_id>?token=", "GET /snapshot/latest?token="] }),
    )
}

async fn health(State(app): State<App>) -> impl IntoResponse {
    Json(
        serde_json::json!({ "ok": true, "version": hive_core::VERSION, "blobs": app.store.count().unwrap_or(0), "used_bytes": app.store.used_bytes().unwrap_or(0), "coordinator": app.is_coordinator.load(Ordering::Relaxed), "live_subscribers": app.live.subscribers().await, "snapshot_age_secs": app.snapshot.age_secs().await }),
    )
}

/// `PUT /a` with the raw bytes; headers: `Authorization: Bearer <node key>` (any paired node),
/// optional `Content-Type`, `X-Hive-Project`, `X-Hive-Card`, `X-Hive-Kind`. Returns `{hash, bytes, url}`.
async fn put_blob(State(app): State<App>, headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let Some(uploader) = verify_uploader(&app, &headers).await else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized: Bearer <node key> required" })),
        )
            .into_response();
    };
    if body.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "empty body" })),
        )
            .into_response();
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let kind = headers
        .get("x-hive-kind")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("output")
        .to_string();
    let project_id = headers
        .get("x-hive-project")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());
    let card_id = headers
        .get("x-hive-card")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());
    let (hash, bytes) = match app.store.put(&body, &mime) {
        Ok(x) => x,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };
    let ann = ArtifactAnnounce {
        hash: hash.clone(),
        bytes,
        mime: mime.clone(),
        kind,
        project_id,
        card_id,
        uploaded_by: Some(uploader),
    };
    if let Err(e) = app.hub.artifact_announce(&ann).await {
        tracing::warn!(%hash, "stored but announce failed: {e}");
    }
    (StatusCode::CREATED, Json(serde_json::json!({ "hash": hash, "bytes": bytes, "mime": mime, "path": format!("/a/{hash}") }))).into_response()
}

async fn get_blob(State(app): State<App>, Path(hash): Path<String>) -> impl IntoResponse {
    if !store::valid_hash(&hash) {
        return (StatusCode::BAD_REQUEST, "bad hash").into_response();
    }
    app.connections.fetch_add(1, Ordering::Relaxed);
    let r = match app.store.get(&hash) {
        Ok(Some((data, mime))) => (
            [
                (header::CONTENT_TYPE, mime),
                (
                    header::CACHE_CONTROL,
                    "public, max-age=31536000, immutable".into(),
                ),
                (header::ETAG, format!("\"{hash}\"")),
            ],
            data,
        )
            .into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "no such artifact").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    app.connections.fetch_sub(1, Ordering::Relaxed);
    r
}

async fn head_blob(State(app): State<App>, Path(hash): Path<String>) -> impl IntoResponse {
    match app.store.stat(&hash) {
        Ok(Some((bytes, mime))) => (
            [
                (header::CONTENT_TYPE, mime),
                (header::CONTENT_LENGTH, bytes.to_string()),
            ],
            StatusCode::OK,
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn verify_uploader(app: &App, headers: &HeaderMap) -> Option<uuid::Uuid> {
    let key = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?
        .trim()
        .to_string();
    verify_node_key(app, &key).await
}

/// Verify a node key against the hub (cached 5 min). Returns the node id.
async fn verify_node_key(app: &App, key: &str) -> Option<uuid::Uuid> {
    let key = key.to_string();
    {
        let cache = app.verified.lock().await;
        if let Some((id, at)) = cache.get(&key) {
            if at.elapsed() < Duration::from_secs(300) {
                return Some(*id);
            }
        }
    }
    let who = app.hub.whoami_for(&key).await.ok()?;
    app.verified
        .lock()
        .await
        .insert(key, (who.node_id, Instant::now()));
    Some(who.node_id)
}

/// `~/.local/share/ohhive/blobs` (macOS: `~/Library/Application Support/ohhive/blobs`).
pub fn default_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ohhive")
        .join("blobs")
}

fn b_hour(b: &backup::Backup) -> u32 {
    b.hour_utc()
}
