//! OH Hive desktop shell (ADR-010). Links `ohhive-core` in-process — the same worker loop as
//! `hive work` — and exposes it to the React UI over Tauri IPC. No second daemon, no sidecar.
//!
//! Surface: pair this machine (code + link to ohghive.com/pair), start/stop working, pick the
//! model, see what the node is doing and what it has earned, tray icon with the same controls,
//! launch at login, and Preferences → About (Happy Jack Media house rule).

use ohhive_core::backend::llama_cpp::LlamaCppBackend;
use ohhive_core::backend::Backend;
use ohhive_core::capability::{Capabilities, Modality, ToolsLevel};
use ohhive_core::hub::{HubClient, Pairing, PairingPoll};
use ohhive_core::nodeconfig::{self, NodeConfig};
use ohhive_core::worker::{Worker, WorkerEvent};
use serde::Serialize;
use std::collections::VecDeque;
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
            allow_internet: false,
            tools_level: ToolsLevel::SandboxedTools,
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
async fn snapshot(state: State<'_, AppState>) -> Result<Snapshot, String> {
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
    })
}

#[tauri::command]
async fn set_config(key: String, value: String) -> Result<String, String> {
    if !key.starts_with("HIVE_") || key == "HIVE_NODE_KEY" {
        return Err("only HIVE_* settings (not the node key) can be changed here".into());
    }
    let p = nodeconfig::set(&key, &value).map_err(|e| e.to_string())?;
    if value.trim().is_empty() {
        std::env::remove_var(&key);
    } else {
        std::env::set_var(&key, value.trim());
    }
    Ok(p.display().to_string())
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
                    ..
                }) => {
                    match nodeconfig::set("HIVE_NODE_KEY", &node_key) {
                        Ok(_) => {
                            std::env::set_var("HIVE_NODE_KEY", &node_key);
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
            let w = Worker {
                hub: &hub,
                backend: &be,
                caps: &caps,
                default_model: model,
                stop: stop_rx,
                events: Some(events),
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
async fn worker_stop(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(tx) = state.worker_stop.lock().await.as_ref() {
        let _ = tx.send(true);
        log(&app, "info", "stopping — releasing any leased card").await;
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
        WorkerEvent::Idle => (String::new(), "", "working, idle"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
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
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
            show_window
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
                                    worker_stop(app.clone(), app.state()).await
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
                                let _ = worker_stop(app.clone(), app.state()).await;
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
