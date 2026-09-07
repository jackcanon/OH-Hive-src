//! OH Hive desktop shell (ADR-010). Links `ohhive-core` in-process — the same worker loop as
//! `hive work` — and exposes it to the React UI over Tauri IPC. No second daemon, no sidecar.
//!
//! Surface: pair this machine (code + link to ohghive.com/pair), start/stop working, pick the
//! model, see what the node is doing and what it has earned, tray icon with the same controls,
//! launch at login, and Preferences → About (Happy Jack Media house rule).
//! v2: first-run setup (`setup.rs` — hardware assessment, Ollama install, model ladder + pull) and the
//! regional-server role in-process (`hive_server::serve`), each its own section.

mod setup;
mod tunnel;

use hive_server::{ServeOptions, ServerStatus};
use ohhive_core::backend::llama_cpp::LlamaCppBackend;
use ohhive_core::backend::Backend;
use ohhive_core::capability::{Capabilities, Modality, ToolsLevel};
use ohhive_core::hub::{HubClient, Pairing, PairingPoll};
use ohhive_core::nodeconfig::{self, NodeConfig};
use ohhive_core::worker::{Worker, WorkerEvent};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tokio::sync::{broadcast, watch, Mutex};

/// Data for Preferences → About. Crediting Happy Jack Media and linking
/// This Is Not A Draft is a house rule and part of the definition of done.
#[derive(Serialize, Clone)]
pub struct AboutInfo {
    pub app_version: &'static str,
    pub core_version: &'static str,
    pub made_by: &'static str,
    pub made_by_url: &'static str,
    pub blog_name: &'static str,
    pub blog_url: &'static str,
}

#[derive(Serialize, Clone)]
struct Activity {
    at: String,
    text: String,
    kind: String,
}

#[derive(Serialize, Clone)]
struct Snapshot {
    version: &'static str,
    paired: bool,
    config_path: String,
    hub_url: String,
    llama_url: String,
    region: Option<String>,
    model: Option<String>,
    models: Vec<String>,
    backend_ok: bool,
    running: bool,
    busy: bool,
    pairing: Option<PairingView>,
    summary: Option<serde_json::Value>,
    activity: Vec<Activity>,
    error: Option<String>,
    server: ServerView,
    worker_enabled: bool,
    server_enabled: bool,
    setup_done: bool,
    allow_internet: bool,
    tools_level: &'static str,
    tunnel: tunnel::TunnelView,
}

#[derive(Serialize, Clone, Default)]
struct ServerView {
    running: bool,
    registered: bool,
    coordinator: bool,
    coordinator_name: Option<String>,
    blobs: u64,
    used_bytes: u64,
    last_backup: Option<String>,
    public_url: String,
    storage_gb: u32,
    tier: String,
    operator: String,
    listen: String,
    data_dir: String,
}

#[derive(Serialize, Clone)]
struct PairingView {
    code: String,
    url: String,
    expires_in_seconds: u64,
}

struct AppState {
    worker_stop: Mutex<Option<watch::Sender<bool>>>,
    /// true while a card is leased (for the tray line)
    busy: Mutex<bool>,
    events: broadcast::Sender<WorkerEvent>,
    activity: Mutex<VecDeque<Activity>>,
    pairing: Mutex<Option<(PairingView, watch::Sender<bool>)>>,
    last_error: Mutex<Option<String>>,
    tray_status: Mutex<Option<MenuItem<tauri::Wry>>>,
    tray_toggle: Mutex<Option<MenuItem<tauri::Wry>>>,
    server_stop: Mutex<Option<watch::Sender<bool>>>,
    server_status: Arc<ServerStatus>,
    setup_busy: Mutex<bool>,
    /// `cloudflared tunnel run`, alive exactly while the regional server role is on and a tunnel
    /// is configured. Owned here so server_stop can kill it alongside hive-server.
    tunnel_child: Mutex<Option<tokio::process::Child>>,
}

fn env_flag(k: &str) -> bool {
    matches!(
        std::env::var(k).as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

fn env_or(k: &str, d: &str) -> String {
    std::env::var(k)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| d.to_string())
}

async fn server_view(state: &AppState) -> ServerView {
    let st = &state.server_status;
    ServerView {
        running: state.server_stop.lock().await.is_some(),
        registered: st.registered.load(std::sync::atomic::Ordering::Relaxed),
        coordinator: st.coordinator.load(std::sync::atomic::Ordering::Relaxed),
        coordinator_name: st.coordinator_name.lock().await.clone(),
        blobs: st.blobs.load(std::sync::atomic::Ordering::Relaxed),
        used_bytes: st.used_bytes.load(std::sync::atomic::Ordering::Relaxed),
        last_backup: st.last_backup.lock().await.clone(),
        public_url: env_or("HIVE_PUBLIC_URL", ""),
        storage_gb: env_or("HIVE_STORAGE_GB", "50").parse().unwrap_or(50),
        tier: env_or("HIVE_TIER", "primary"),
        operator: env_or("HIVE_OPERATOR", "volunteer"),
        listen: env_or("HIVE_LISTEN", "0.0.0.0:8790"),
        data_dir: std::env::var("HIVE_DATA_DIR")
            .unwrap_or_else(|_| hive_server::default_data_dir().display().to_string()),
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

async fn log(app: &AppHandle, kind: &str, text: impl Into<String>) {
    let st = app.state::<AppState>();
    let a = Activity {
        at: now_iso(),
        text: text.into(),
        kind: kind.into(),
    };
    let mut q = st.activity.lock().await;
    q.push_front(a.clone());
    q.truncate(60);
    let _ = app.emit("activity", a);
}

async fn set_tray(app: &AppHandle, status: &str) {
    let st = app.state::<AppState>();
    let running = st.worker_stop.lock().await.is_some();
    let status_item = st.tray_status.lock().await;
    if let Some(item) = status_item.as_ref() {
        let _ = item.set_text(format!("OH Hive — {status}"));
    }
    let toggle_item = st.tray_toggle.lock().await;
    if let Some(item) = toggle_item.as_ref() {
        let _ = item.set_text(if running {
            "Stop working"
        } else {
            "Start working"
        });
    }
}

async fn capabilities(cfg: &NodeConfig) -> (Capabilities, bool) {
    let hardware = ohhive_core::probe::probe_hardware();
    let be = LlamaCppBackend::new(&cfg.llama_url);
    let (mut modalities, mut models, ok) = match be.capabilities().await {
        Ok(c) => (c.modalities, c.models, true),
        Err(e) => {
            tracing::warn!("backend at {} unavailable: {e}", cfg.llama_url);
            (vec![], vec![], false)
        }
    };
    if modalities.is_empty() {
        modalities.push(Modality::Text);
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    (
        Capabilities {
            hardware,
            modalities,
            models,
            // ADR-006 D46/D48: read from node.env, seeded from the pairing choice and
            // changeable afterward in Settings → Trust. Never hardcode these — check-in
            // sends whatever is here, overwriting the hub's row (see hive.node_checkin).
            allow_internet: cfg.allow_internet,
            tools_level: cfg.tools_level,
            storage_gb_offered: None,
            shard_capable: None,
        },
        ok,
    )
}

fn model_pref() -> Option<String> {
    std::env::var("HIVE_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[tauri::command]
fn about() -> AboutInfo {
    AboutInfo {
        app_version: env!("CARGO_PKG_VERSION"),
        core_version: ohhive_core::VERSION,
        made_by: "Happy Jack Media",
        made_by_url: "https://happyjack.media",
        blog_name: "This Is Not A Draft",
        blog_url: "https://thisisnotadraft.com",
    }
}

#[tauri::command]
async fn snapshot(app: AppHandle, state: State<'_, AppState>) -> Result<Snapshot, String> {
    nodeconfig::export_env();
    let cfg = nodeconfig::load().map_err(|e| e.to_string())?;
    let (caps, backend_ok) = capabilities(&cfg).await;
    let summary = match &cfg.node_key {
        Some(k) => {
            let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, k.clone());
            match hub.node_summary().await {
                Ok(v) => Some(v),
                Err(e) => {
                    *state.last_error.lock().await = Some(format!("hub: {e}"));
                    None
                }
            }
        }
        None => None,
    };
    Ok(Snapshot {
        version: env!("CARGO_PKG_VERSION"),
        paired: cfg.node_key.is_some(),
        config_path: nodeconfig::path().display().to_string(),
        hub_url: cfg.hub_url.clone(),
        llama_url: cfg.llama_url.clone(),
        region: cfg.region.clone(),
        model: model_pref(),
        models: caps.models.iter().map(|m| m.id.clone()).collect(),
        backend_ok,
        running: state.worker_stop.lock().await.is_some(),
        busy: *state.busy.lock().await,
        pairing: state.pairing.lock().await.as_ref().map(|(v, _)| v.clone()),
        summary,
        activity: state.activity.lock().await.iter().cloned().collect(),
        error: state.last_error.lock().await.take(),
        server: server_view(&state).await,
        worker_enabled: env_flag("HIVE_WORKER_ENABLED"),
        server_enabled: env_flag("HIVE_SERVER_ENABLED"),
        setup_done: cfg.node_key.is_some() && env_flag("HIVE_SETUP_DONE"),
        allow_internet: cfg.allow_internet,
        tools_level: match cfg.tools_level {
            ToolsLevel::InferenceOnly => "inference_only",
            ToolsLevel::SandboxedTools => "sandboxed_tools",
        },
        tunnel: tunnel::TunnelView {
            available: tunnel::bundled_path(&app).is_some(),
            logged_in: tunnel::logged_in(),
            hostname: std::env::var("HIVE_TUNNEL_HOSTNAME").ok(),
            running: state.tunnel_child.lock().await.is_some(),
        },
    })
}

// ---------- setup (first run) ----------

async fn ladder(cfg: &NodeConfig) -> Vec<setup::Rung> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let r = http
        .post(format!("{}/rest/v1/rpc/hive_model_ladder", cfg.hub_url))
        .header("apikey", &cfg.anon_key)
        .header("Authorization", format!("Bearer {}", cfg.anon_key))
        .json(&serde_json::json!({}))
        .send()
        .await;
    if let Ok(r) = r {
        if r.status().is_success() {
            if let Ok(v) = r.json::<Vec<setup::Rung>>().await {
                if !v.is_empty() {
                    return v;
                }
            }
        }
    }
    setup::builtin_ladder()
}

#[tauri::command]
async fn assess() -> Result<setup::Assessment, String> {
    nodeconfig::export_env();
    let cfg = nodeconfig::load().map_err(|e| e.to_string())?;
    let l = ladder(&cfg).await;
    Ok(setup::assess(&cfg.llama_url, &l).await)
}

fn reporter(app: AppHandle) -> impl Fn(setup::Progress) + Send + Sync + 'static {
    move |p| {
        let _ = app.emit("setup", &p);
    }
}

#[tauri::command]
async fn ollama_install(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut b = state.setup_busy.lock().await;
        if *b {
            return Err("setup step already running".into());
        }
        *b = true;
    }
    log(&app, "info", "installing Ollama").await;
    let r = setup::install_ollama(reporter(app.clone())).await;
    *state.setup_busy.lock().await = false;
    match r {
        Ok(()) => {
            log(&app, "ok", "Ollama installed and running").await;
            Ok(())
        }
        Err(e) => {
            let _ = app.emit(
                "setup",
                setup::Progress {
                    phase: "install".into(),
                    text: e.to_string(),
                    completed: 0,
                    total: 0,
                    done: true,
                    error: Some(e.to_string()),
                },
            );
            log(&app, "error", format!("Ollama install: {e}")).await;
            Err(e.to_string())
        }
    }
}

#[tauri::command]
async fn ollama_pull(
    app: AppHandle,
    state: State<'_, AppState>,
    model: String,
) -> Result<(), String> {
    {
        let mut b = state.setup_busy.lock().await;
        if *b {
            return Err("setup step already running".into());
        }
        *b = true;
    }
    nodeconfig::export_env();
    let cfg = nodeconfig::load().map_err(|e| e.to_string())?;
    log(&app, "info", format!("pulling {model}")).await;
    let r = setup::pull_model(&cfg.llama_url, &model, reporter(app.clone())).await;
    *state.setup_busy.lock().await = false;
    match r {
        Ok(()) => {
            let _ = nodeconfig::set("HIVE_MODEL", &model);
            std::env::set_var("HIVE_MODEL", &model);
            log(
                &app,
                "ok",
                format!("{model} ready — it's now this machine's model"),
            )
            .await;
            Ok(())
        }
        Err(e) => {
            let _ = app.emit(
                "setup",
                setup::Progress {
                    phase: "pull".into(),
                    text: e.to_string(),
                    completed: 0,
                    total: 0,
                    done: true,
                    error: Some(e.to_string()),
                },
            );
            log(&app, "error", format!("pull {model}: {e}")).await;
            Err(e.to_string())
        }
    }
}

#[tauri::command]
async fn setup_finish() -> Result<(), String> {
    nodeconfig::set("HIVE_SETUP_DONE", "1").map_err(|e| e.to_string())?;
    std::env::set_var("HIVE_SETUP_DONE", "1");
    Ok(())
}

// ---------- regional server (in-process hive-server) ----------

#[tauri::command]
async fn server_start(
    app: AppHandle,
    state: State<'_, AppState>,
    public_url: Option<String>,
    storage_gb: Option<u32>,
    tier: Option<String>,
) -> Result<(), String> {
    if state.server_stop.lock().await.is_some() {
        return Ok(());
    }
    nodeconfig::export_env();
    if let Some(u) = public_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        nodeconfig::set("HIVE_PUBLIC_URL", u).map_err(|e| e.to_string())?;
        std::env::set_var("HIVE_PUBLIC_URL", u);
    }
    if let Some(g) = storage_gb {
        nodeconfig::set("HIVE_STORAGE_GB", &g.to_string()).map_err(|e| e.to_string())?;
        std::env::set_var("HIVE_STORAGE_GB", g.to_string());
    }
    if let Some(t) = tier
        .as_deref()
        .filter(|t| *t == "primary" || *t == "standby")
    {
        nodeconfig::set("HIVE_TIER", t).map_err(|e| e.to_string())?;
        std::env::set_var("HIVE_TIER", t);
    }
    let cfg = nodeconfig::load().map_err(|e| e.to_string())?;
    let key = cfg.node_key.clone().ok_or("pair this machine first")?;
    let opts = ServeOptions {
        public_url: env_or("HIVE_PUBLIC_URL", ""),
        listen: env_or("HIVE_LISTEN", "0.0.0.0:8790"),
        data_dir: std::env::var("HIVE_DATA_DIR")
            .ok()
            .map(std::path::PathBuf::from),
        storage_gb: env_or("HIVE_STORAGE_GB", "50").parse().unwrap_or(50),
        operator: env_or("HIVE_OPERATOR", "volunteer"),
        tier: env_or("HIVE_TIER", "primary"),
        region: cfg.region.clone(),
        max_upload_mb: env_or("HIVE_MAX_UPLOAD_MB", "512").parse().unwrap_or(512),
        backup_recipient: std::env::var("HIVE_BACKUP_RECIPIENT")
            .ok()
            .filter(|s| !s.trim().is_empty()),
        backup_hour_utc: env_or("HIVE_BACKUP_HOUR_UTC", "9").parse().unwrap_or(9),
    };
    if opts.public_url.is_empty() {
        return Err(
            "set the public URL first (a Cloudflare Tunnel hostname or http://<public-ip>:8790)"
                .into(),
        );
    }
    // If Tunnel setup has run, bring the tunnel up alongside the server so the public URL it
    // configured is actually reachable. Not fatal if it fails to start -- a manually-run
    // cloudflared, or a real public IP, still works; server_start only needed opts.public_url.
    if state.tunnel_child.lock().await.is_none() && std::env::var("HIVE_TUNNEL_ID").is_ok() {
        if let Some(bin) = tunnel::bundled_path(&app) {
            match tunnel::spawn_run(&bin) {
                Ok(child) => {
                    *state.tunnel_child.lock().await = Some(child);
                    log(&app, "ok", "cloudflare tunnel connecting").await;
                }
                Err(e) => log(&app, "error", format!("tunnel did not start: {e}")).await,
            }
        }
    }
    let (stop_tx, mut stop_rx) = watch::channel(false);
    *state.server_stop.lock().await = Some(stop_tx);
    let _ = nodeconfig::set("HIVE_SERVER_ENABLED", "1");
    std::env::set_var("HIVE_SERVER_ENABLED", "1");
    let status = state.server_status.clone();
    let hub = Arc::new(HubClient::new(&cfg.hub_url, &cfg.anon_key, key));
    log(
        &app,
        "ok",
        format!(
            "regional server starting at {} (storage {} GB, {})",
            opts.public_url, opts.storage_gb, opts.tier
        ),
    )
    .await;
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let stop = async move {
            while !*stop_rx.borrow() {
                if stop_rx.changed().await.is_err() {
                    break;
                }
            }
        };
        let r = hive_server::serve(&cfg, hub, opts, stop, status.clone()).await;
        match r {
            Ok(()) => log(&app2, "info", "regional server stopped — checked out").await,
            Err(e) => log(&app2, "error", format!("regional server stopped: {e:#}")).await,
        }
        status
            .registered
            .store(false, std::sync::atomic::Ordering::Relaxed);
        status
            .coordinator
            .store(false, std::sync::atomic::Ordering::Relaxed);
        *app2.state::<AppState>().server_stop.lock().await = None;
        let _ = app2.emit("changed", ());
    });
    Ok(())
}

#[tauri::command]
async fn server_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    forget: Option<bool>,
) -> Result<(), String> {
    if let Some(tx) = state.server_stop.lock().await.as_ref() {
        let _ = tx.send(true);
        log(&app, "info", "stopping regional server").await;
    }
    if let Some(mut child) = state.tunnel_child.lock().await.take() {
        let _ = child.kill().await;
    }
    if forget.unwrap_or(true) {
        let _ = nodeconfig::set("HIVE_SERVER_ENABLED", "0");
        std::env::set_var("HIVE_SERVER_ENABLED", "0");
    }
    Ok(())
}

/// `HIVE_DATA_DIR` (where regional-server blobs live) gets its own check, since a typo or a
/// path on a drive that isn't actually mounted would otherwise only surface later, as a
/// confusing failure deep inside `hive_server::serve`. Creates the directory if it doesn't
/// exist yet (picking a fresh empty folder is the normal case), then proves it's writable by
/// this process with a real probe file rather than trusting permission bits alone (network
/// shares and some external drives lie about those).
fn validate_data_dir(path: &str) -> Result<(), String> {
    let dir = std::path::Path::new(path.trim());
    std::fs::create_dir_all(dir).map_err(|e| format!("can't use {path} as storage: {e}"))?;
    let probe = dir.join(".ohhive-write-test");
    std::fs::write(&probe, b"ok").map_err(|e| format!("{path} isn't writable by OH Hive: {e}"))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

#[tauri::command]
async fn set_config(key: String, value: String) -> Result<String, String> {
    if !key.starts_with("HIVE_") || key == "HIVE_NODE_KEY" {
        return Err("only HIVE_* settings (not the node key) can be changed here".into());
    }
    if key == "HIVE_DATA_DIR" && !value.trim().is_empty() {
        validate_data_dir(&value)?;
    }
    let p = nodeconfig::set(&key, &value).map_err(|e| e.to_string())?;
    if value.trim().is_empty() {
        std::env::remove_var(&key);
    } else {
        std::env::set_var(&key, value.trim());
    }
    Ok(p.display().to_string())
}

// ---------- Cloudflare Tunnel (ADR-013 D74) ----------

#[tauri::command]
async fn tunnel_login(app: AppHandle) -> Result<(), String> {
    let bin = tunnel::bundled_path(&app)
        .ok_or("this build has no bundled cloudflared -- Tunnel setup is unavailable")?;
    log(&app, "info", "opening Cloudflare login in your browser").await;
    let opener = move |url: String| {
        let _ = std::process::Command::new("open").arg(url).spawn();
    };
    tunnel::login(&bin, opener)
        .await
        .map_err(|e| e.to_string())?;
    log(&app, "ok", "Cloudflare account connected").await;
    Ok(())
}

/// Create (or find) a tunnel named `<name>-hive`, route `hostname` to it, write its config, and
/// save enough to node.env that server_start can bring the tunnel up and set the public URL.
/// Does not start it running -- that happens the next time the regional server role starts.
#[tauri::command]
async fn tunnel_setup(app: AppHandle, name: String, hostname: String) -> Result<String, String> {
    let bin = tunnel::bundled_path(&app)
        .ok_or("this build has no bundled cloudflared -- Tunnel setup is unavailable")?;
    if !tunnel::logged_in() {
        return Err("connect your Cloudflare account first".into());
    }
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        return Err("give this machine a short name for the tunnel".into());
    }
    let created = tunnel::create(&bin, &name)
        .await
        .map_err(|e| e.to_string())?;
    tunnel::route_dns(&bin, &name, &hostname)
        .await
        .map_err(|e| e.to_string())?;
    tunnel::write_config(&created.id, &created.credentials_file, &hostname)
        .map_err(|e| e.to_string())?;
    let public_url = format!("https://{hostname}");
    for (k, v) in [
        ("HIVE_TUNNEL_NAME", name.as_str()),
        ("HIVE_TUNNEL_ID", created.id.as_str()),
        ("HIVE_TUNNEL_HOSTNAME", hostname.as_str()),
        ("HIVE_PUBLIC_URL", public_url.as_str()),
    ] {
        nodeconfig::set(k, v).map_err(|e| e.to_string())?;
        std::env::set_var(k, v);
    }
    log(&app, "ok", format!("tunnel ready at {public_url}")).await;
    Ok(public_url)
}

#[tauri::command]
async fn pair_begin(app: AppHandle, state: State<'_, AppState>) -> Result<PairingView, String> {
    let cfg = nodeconfig::load().map_err(|e| e.to_string())?;
    if cfg.node_key.is_some() {
        return Err("this machine is already paired".into());
    }
    if let Some((v, _)) = state.pairing.lock().await.as_ref() {
        return Ok(v.clone());
    }
    let hw = ohhive_core::probe::probe_hardware();
    let hint = serde_json::json!({
        "hostname": hostname(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "cpu": hw.cpu_model,
        "gpu": hw.gpu_model,
        "ram_gb": (hw.ram_bytes as f64 / 1e9).round(),
        "app": "desktop",
    });
    let p = Pairing::new(&cfg.hub_url, &cfg.anon_key);
    let start = p.begin(hint).await.map_err(|e| e.to_string())?;
    let view = PairingView {
        code: start.code.clone(),
        url: start.url.clone(),
        expires_in_seconds: start.expires_in_seconds,
    };
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    *state.pairing.lock().await = Some((view.clone(), cancel_tx));
    log(
        &app,
        "info",
        format!("pairing code {} — waiting for ohghive.com/pair", start.code),
    )
    .await;
    let secret = start.secret.clone();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(3));
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = cancel_rx.changed() => { break; }
            }
            match p.poll(&secret).await {
                Ok(PairingPoll::Pending) => continue,
                Ok(PairingPoll::Expired) => {
                    log(&app2, "error", "pairing code expired — start again").await;
                    break;
                }
                Ok(PairingPoll::Claimed {
                    node_key,
                    display_name,
                    allow_internet,
                    tools_level,
                    ..
                }) => {
                    match nodeconfig::set("HIVE_NODE_KEY", &node_key) {
                        Ok(_) => {
                            std::env::set_var("HIVE_NODE_KEY", &node_key);
                            // Seed local config with what was chosen on the pairing page —
                            // otherwise the first check-in would silently reset both to their
                            // defaults (see hive.node_checkin's coalesce()).
                            let allow_internet_s = if allow_internet { "true" } else { "false" };
                            let _ = nodeconfig::set("HIVE_ALLOW_INTERNET", allow_internet_s);
                            std::env::set_var("HIVE_ALLOW_INTERNET", allow_internet_s);
                            let tools_level_s = match tools_level {
                                ToolsLevel::InferenceOnly => "inference_only",
                                ToolsLevel::SandboxedTools => "sandboxed_tools",
                            };
                            let _ = nodeconfig::set("HIVE_TOOLS_LEVEL", tools_level_s);
                            std::env::set_var("HIVE_TOOLS_LEVEL", tools_level_s);
                            log(&app2, "ok", format!("paired as “{display_name}”")).await;
                        }
                        Err(e) => {
                            log(&app2, "error", format!("could not save node key: {e}")).await
                        }
                    }
                    break;
                }
                Err(e) => {
                    log(&app2, "error", format!("pairing poll failed: {e}")).await;
                    break;
                }
            }
        }
        *app2.state::<AppState>().pairing.lock().await = None;
        let _ = app2.emit("changed", ());
    });
    Ok(view)
}

#[tauri::command]
async fn pair_cancel(state: State<'_, AppState>) -> Result<(), String> {
    if let Some((_, tx)) = state.pairing.lock().await.take() {
        let _ = tx.send(true);
    }
    Ok(())
}

#[tauri::command]
async fn worker_start(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.worker_stop.lock().await.is_some() {
        return Ok(());
    }
    nodeconfig::export_env();
    let cfg = nodeconfig::load().map_err(|e| e.to_string())?;
    let key = cfg.node_key.clone().ok_or("pair this machine first")?;
    let (caps, ok) = capabilities(&cfg).await;
    if !ok || caps.models.is_empty() {
        return Err(format!(
            "no models available at {} — is Ollama running?",
            cfg.llama_url
        ));
    }
    let (stop_tx, stop_rx) = watch::channel(false);
    *state.worker_stop.lock().await = Some(stop_tx);
    let _ = nodeconfig::set("HIVE_WORKER_ENABLED", "1");
    std::env::set_var("HIVE_WORKER_ENABLED", "1");
    let events = state.events.clone();
    let model = model_pref();
    let app2 = app.clone();
    log(
        &app,
        "ok",
        format!(
            "working — {} models, model {}",
            caps.models.len(),
            model.clone().unwrap_or_else(|| "auto".into())
        ),
    )
    .await;
    set_tray(&app, "working, idle").await;
    tauri::async_runtime::spawn(async move {
        let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
        let be = LlamaCppBackend::new(&cfg.llama_url);
        let r: anyhow::Result<()> = async {
            hub.check_in(&caps, cfg.region.as_deref()).await?;
            // Desktop has no `HIVE_DATA_DIR` of its own for the compute-worker role today (that
            // env var is the regional-server role's, ADR-004) — reuse the same sandbox default
            // the headless `hive work` CLI uses rather than invent a second data directory.
            let sandbox = ohhive_core::sandbox::Sandbox::new()
                .map_err(|e| anyhow::anyhow!("sandbox engine init failed: {e}"))?;
            let w = Worker {
                hub: &hub,
                backend: &be,
                caps: &caps,
                default_model: model,
                stop: stop_rx,
                events: Some(events),
                data_dir: ohhive_core::sandbox::default_data_dir(),
                sandbox: Some(&sandbox),
            };
            w.run_forever(std::time::Duration::from_secs(5), 6).await
        }
        .await;
        if let Err(e) = r {
            log(&app2, "error", format!("worker stopped: {e}")).await;
            let _ = hub.check_out().await;
        } else {
            log(&app2, "info", "stopped — checked out").await;
        }
        let st = app2.state::<AppState>();
        *st.worker_stop.lock().await = None;
        *st.busy.lock().await = false;
        set_tray(&app2, "stopped").await;
        let _ = app2.emit("changed", ());
    });
    Ok(())
}

#[tauri::command]
async fn worker_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    forget: Option<bool>,
) -> Result<(), String> {
    if let Some(tx) = state.worker_stop.lock().await.as_ref() {
        let _ = tx.send(true);
        log(&app, "info", "stopping — releasing any leased card").await;
    }
    if forget.unwrap_or(true) {
        let _ = nodeconfig::set("HIVE_WORKER_ENABLED", "0");
        std::env::set_var("HIVE_WORKER_ENABLED", "0");
    }
    Ok(())
}

#[tauri::command]
async fn show_window(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
    Ok(())
}

fn hostname() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn describe(e: &WorkerEvent) -> (String, &'static str, &'static str) {
    match e {
        WorkerEvent::Leased {
            card,
            project,
            resume,
        } => (
            format!(
                "{}“{card}” for {project}",
                if *resume { "resuming " } else { "took " }
            ),
            "info",
            "working on a card",
        ),
        WorkerEvent::Step {
            card,
            step,
            next,
            tokens_out,
            ..
        } => (
            format!("“{card}” step {step} done → {next} ({tokens_out} tokens so far)"),
            "info",
            "working on a card",
        ),
        WorkerEvent::Completed {
            card,
            earned_honey,
            tokens_out,
            ..
        } => (
            format!("finished “{card}” — earned {earned_honey:.4} Honey ({tokens_out} tokens)"),
            "ok",
            "working, idle",
        ),
        WorkerEvent::Failed { card, error } => (
            format!("“{card}” failed: {error}"),
            "error",
            "working, idle",
        ),
        WorkerEvent::Released { card } => (
            format!("released “{card}” back to the board"),
            "info",
            "stopped",
        ),
        WorkerEvent::Blocked { card, waiting_on } => (
            format!("“{card}” is waiting on its spawned card “{waiting_on}”"),
            "info",
            "working, idle",
        ),
        WorkerEvent::Idle => (String::new(), "", "working, idle"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Kept alive for the whole process: dropping it early would silently truncate the log file.
    // This is the one binary where that file is the *only* log a member can hand back to us --
    // there's no attached terminal to catch stdout when it's launched by double-click.
    let _log_guard = ohhive_core::logging::init("desktop");
    nodeconfig::export_env();
    let (events, _) = broadcast::channel::<WorkerEvent>(64);
    let state = AppState {
        worker_stop: Mutex::new(None),
        busy: Mutex::new(false),
        events: events.clone(),
        activity: Mutex::new(VecDeque::new()),
        pairing: Mutex::new(None),
        last_error: Mutex::new(None),
        tray_status: Mutex::new(None),
        tray_toggle: Mutex::new(None),
        server_stop: Mutex::new(None),
        server_status: Arc::new(ServerStatus::default()),
        setup_busy: Mutex::new(false),
        tunnel_child: Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            about,
            snapshot,
            set_config,
            pair_begin,
            pair_cancel,
            worker_start,
            worker_stop,
            show_window,
            assess,
            ollama_install,
            ollama_pull,
            setup_finish,
            server_start,
            server_stop,
            tunnel_login,
            tunnel_setup
        ])
        .on_window_event(|w, e| {
            // Closing the window keeps the node working; the tray icon brings it back.
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();

            // tray
            let status =
                MenuItem::with_id(app, "status", "OH Hive — stopped", false, None::<&str>)?;
            let toggle = MenuItem::with_id(app, "toggle", "Start working", true, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Open OH Hive", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit OH Hive", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &status,
                    &PredefinedMenuItem::separator(app)?,
                    &toggle,
                    &open,
                    &PredefinedMenuItem::separator(app)?,
                    &quit,
                ],
            )?;
            {
                let st = app.state::<AppState>();
                *st.tray_status.blocking_lock() = Some(status);
                *st.tray_toggle.blocking_lock() = Some(toggle);
            }
            let mut tray = TrayIconBuilder::with_id("main")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .tooltip("OH Hive")
                .on_menu_event(|app, ev| {
                    let app = app.clone();
                    match ev.id().as_ref() {
                        "toggle" => {
                            tauri::async_runtime::spawn(async move {
                                let st = app.state::<AppState>();
                                let running = st.worker_stop.lock().await.is_some();
                                let r = if running {
                                    worker_stop(app.clone(), app.state(), None).await
                                } else {
                                    worker_start(app.clone(), app.state()).await
                                };
                                if let Err(e) = r {
                                    log(&app, "error", e).await;
                                    let _ = show_window(app.clone()).await;
                                }
                                let _ = app.emit("changed", ());
                            });
                        }
                        "open" => {
                            tauri::async_runtime::spawn(async move {
                                let _ = show_window(app).await;
                            });
                        }
                        "quit" => {
                            tauri::async_runtime::spawn(async move {
                                let _ = worker_stop(app.clone(), app.state(), Some(false)).await;
                                let _ = server_stop(app.clone(), app.state(), Some(false)).await;
                                // give the worker a moment to release its lease and check out
                                for _ in 0..40 {
                                    if app.state::<AppState>().worker_stop.lock().await.is_none() {
                                        break;
                                    }
                                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                                }
                                app.exit(0);
                            });
                        }
                        _ => {}
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // resume roles the member had on (launch-at-login makes this the normal path)
            {
                let h = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    if env_flag("HIVE_WORKER_ENABLED") {
                        if let Err(e) = worker_start(h.clone(), h.state()).await {
                            log(&h, "error", format!("could not resume working: {e}")).await;
                        }
                    }
                    if env_flag("HIVE_SERVER_ENABLED") {
                        if let Err(e) = server_start(h.clone(), h.state(), None, None, None).await {
                            log(
                                &h,
                                "error",
                                format!("could not resume the regional server: {e}"),
                            )
                            .await;
                        }
                    }
                    let _ = h.emit("changed", ());
                });
            }

            // worker events → activity log + UI + tray line
            let mut rx = events.subscribe();
            tauri::async_runtime::spawn(async move {
                while let Ok(ev) = rx.recv().await {
                    let (text, kind, tray_line) = describe(&ev);
                    let st = handle.state::<AppState>();
                    *st.busy.lock().await =
                        matches!(ev, WorkerEvent::Leased { .. } | WorkerEvent::Step { .. });
                    if !text.is_empty() {
                        log(&handle, kind, text).await;
                    }
                    set_tray(&handle, tray_line).await;
                    let _ = handle.emit("worker", &ev);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running OH Hive");
}
