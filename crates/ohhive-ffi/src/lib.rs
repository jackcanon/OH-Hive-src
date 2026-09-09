//! ohhive-ffi -- UniFFI bridge exposing ohhive-core's phase-1 desktop surface (ADR-018) to the
//! native macOS SwiftUI app: pairing, the compute-node worker, snapshot/about, config, and an
//! activity/change event callback. This mirrors `apps/desktop/src-tauri/src/lib.rs`'s Tauri IPC
//! surface one-for-one for everything phase 1 covers.
//!
//! Explicitly out of scope here (ADR-018 decision 4, deferred to phase 2): the first-run Setup
//! wizard (`setup.rs`), the regional-server role, and Cloudflare Tunnel (`tunnel.rs`) -- none of
//! that is wrapped. A phase-2 pass adds a second `#[uniffi::export] impl` block for those once
//! phase 1 is solid on real hardware.
//!
//! No hand-maintained `.udl` file: every exported type/fn/method is declared with proc-macro
//! attributes right here, and `src/bin/uniffi-bindgen.rs` generates the Swift binding module
//! from this crate directly.

use ohhive_core::backend::llama_cpp::LlamaCppBackend;
use ohhive_core::backend::Backend;
use ohhive_core::capability::{Capabilities, Modality, ToolsLevel};
use ohhive_core::hub::{HubClient, HubError, Pairing, PairingPoll};
use ohhive_core::nodeconfig::{self, NodeConfig};
use ohhive_core::worker::{Worker, WorkerEvent};
use once_cell::sync::Lazy;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{watch, Mutex as AsyncMutex};

uniffi::setup_scaffolding!();

/// One shared multi-threaded Tokio runtime for the whole FFI surface. Every exported async
/// method spawns its real work onto this runtime and awaits the JoinHandle, instead of letting
/// UniFFI's own foreign-future bridge poll tokio/reqwest internals directly -- `HubClient` and
/// `Pairing` use `reqwest`, which needs a live Tokio reactor under whatever task calls it, and
/// there's no guarantee UniFFI's async bridge provides one. Swift never sees any of this; it
/// just gets ordinary Swift `async` functions.
static RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("ohhive-ffi")
        .build()
        .expect("failed to start ohhive-ffi tokio runtime")
});

// ---------- data types crossing the FFI boundary ----------

#[derive(uniffi::Record, Clone)]
pub struct AboutInfo {
    pub app_version: String,
    pub core_version: String,
    pub made_by: String,
    pub made_by_url: String,
    pub blog_name: String,
    pub blog_url: String,
}

#[derive(uniffi::Record, Clone)]
pub struct ActivityEntry {
    pub at: String,
    pub text: String,
    pub kind: String,
}

#[derive(uniffi::Record, Clone)]
pub struct PairingView {
    pub code: String,
    pub url: String,
    pub expires_in_seconds: u64,
}

#[derive(uniffi::Record, Clone)]
pub struct HiveSnapshot {
    pub version: String,
    pub paired: bool,
    pub config_path: String,
    pub hub_url: String,
    pub llama_url: String,
    pub region: Option<String>,
    pub model: Option<String>,
    pub models: Vec<String>,
    pub backend_ok: bool,
    pub running: bool,
    pub busy: bool,
    pub pairing: Option<PairingView>,
    /// `hive_node_summary`'s result is an open-shaped jsonb document. Passed through as a JSON
    /// string for phase 1 rather than modeled field-by-field -- decode what you need on the
    /// Swift side with `JSONSerialization`. Worth a proper `uniffi::Record` once that shape is
    /// stable enough to commit to across the FFI boundary.
    pub summary_json: Option<String>,
    pub activity: Vec<ActivityEntry>,
    pub error: Option<String>,
    pub worker_enabled: bool,
    pub allow_internet: bool,
    pub tools_level: String,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum HiveError {
    #[error("{0}")]
    Failed(String),
}

impl From<HubError> for HiveError {
    fn from(e: HubError) -> Self {
        HiveError::Failed(e.to_string())
    }
}

impl From<anyhow::Error> for HiveError {
    fn from(e: anyhow::Error) -> Self {
        HiveError::Failed(e.to_string())
    }
}

/// Implemented in Swift (`HiveNode.setListener`); receives activity log lines and a generic
/// "something changed, re-read snapshot()" nudge. Mirrors the Tauri app's `activity` / `changed`
/// window events, minus the fine-grained per-step `WorkerEvent` stream -- phase 1 folds those
/// into human-readable activity lines instead (see `describe()` below) rather than modeling the
/// whole `WorkerEvent` enum as FFI types.
#[uniffi::export(callback_interface)]
pub trait HiveEventListener: Send + Sync {
    fn on_activity(&self, entry: ActivityEntry);
    fn on_changed(&self);
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn hostname() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn model_pref() -> Option<String> {
    std::env::var("HIVE_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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
            // changeable afterward in Settings -> Trust. Never hardcode these.
            allow_internet: cfg.allow_internet,
            tools_level: cfg.tools_level,
            storage_gb_offered: None,
            shard_capable: None,
        },
        ok,
    )
}

/// One human-readable line + activity kind per `WorkerEvent`, same idea as the Tauri app's
/// `describe()`.
fn describe(e: &WorkerEvent) -> (String, &'static str) {
    match e {
        WorkerEvent::Leased { card, project, resume } => (
            format!(
                "{} \u{201c}{card}\u{201d} for {project}",
                if *resume { "resuming" } else { "leased" }
            ),
            "info",
        ),
        WorkerEvent::Step { card, step, .. } => (format!("{card}: step {step}"), "info"),
        WorkerEvent::Completed { card, project, earned_honey, .. } => (
            format!("finished \u{201c}{card}\u{201d} for {project} \u{2014} +{earned_honey:.4} honey"),
            "ok",
        ),
        WorkerEvent::Failed { card, error } => (format!("{card} failed: {error}"), "error"),
        WorkerEvent::Released { card } => (format!("released {card}"), "info"),
        WorkerEvent::Blocked { card, waiting_on } => {
            (format!("{card} blocked, waiting on {waiting_on}"), "info")
        }
        WorkerEvent::Idle => ("idle \u{2014} waiting for work".to_string(), "info"),
    }
}

struct PairingHandle {
    view: PairingView,
    cancel: watch::Sender<bool>,
}

/// The FFI surface's one long-lived object. Swift creates exactly one (`HiveNode()`) at launch,
/// holds it for the app's lifetime, and can call its methods from any thread/actor -- everything
/// inside is behind async-aware locks, matching the Tauri app's single shared `AppState`.
#[derive(uniffi::Object)]
pub struct HiveNode {
    worker_stop: AsyncMutex<Option<watch::Sender<bool>>>,
    busy: AsyncMutex<bool>,
    activity: AsyncMutex<VecDeque<ActivityEntry>>,
    pairing: AsyncMutex<Option<PairingHandle>>,
    last_error: AsyncMutex<Option<String>>,
    listener: AsyncMutex<Option<Box<dyn HiveEventListener>>>,
}

impl HiveNode {
    async fn log(&self, kind: &str, text: impl Into<String>) {
        let entry = ActivityEntry {
            at: now_iso(),
            text: text.into(),
            kind: kind.into(),
        };
        {
            let mut q = self.activity.lock().await;
            q.push_front(entry.clone());
            q.truncate(60);
        }
        if let Some(l) = self.listener.lock().await.as_ref() {
            l.on_activity(entry);
        }
    }

    async fn notify_changed(&self) {
        if let Some(l) = self.listener.lock().await.as_ref() {
            l.on_changed();
        }
    }
}

#[uniffi::export]
impl HiveNode {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            worker_stop: AsyncMutex::new(None),
            busy: AsyncMutex::new(false),
            activity: AsyncMutex::new(VecDeque::new()),
            pairing: AsyncMutex::new(None),
            last_error: AsyncMutex::new(None),
            listener: AsyncMutex::new(None),
        }
    }

    /// Swift calls this once at launch (before doing anything else) to receive activity lines
    /// and change notifications.
    pub fn set_listener(&self, listener: Box<dyn HiveEventListener>) {
        RUNTIME.block_on(async {
            *self.listener.lock().await = Some(listener);
        });
    }

    /// Preferences -> About. Crediting Happy Jack Media and linking This Is Not A Draft is a
    /// house rule and part of the definition of done for every app built for this team.
    pub fn about(&self) -> AboutInfo {
        AboutInfo {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            core_version: ohhive_core::VERSION.to_string(),
            made_by: "Happy Jack Media".to_string(),
            made_by_url: "https://happyjack.media".to_string(),
            blog_name: "This Is Not A Draft".to_string(),
            blog_url: "https://thisisnotadraft.com".to_string(),
        }
    }

    pub async fn snapshot(self: Arc<Self>) -> Result<HiveSnapshot, HiveError> {
        RUNTIME
            .spawn(async move {
                nodeconfig::export_env();
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let (caps, backend_ok) = capabilities(&cfg).await;
                let summary_json = match &cfg.node_key {
                    Some(k) => {
                        let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, k.clone());
                        match hub.node_summary().await {
                            Ok(v) => Some(v.to_string()),
                            Err(e) => {
                                *self.last_error.lock().await = Some(format!("hub: {e}"));
                                None
                            }
                        }
                    }
                    None => None,
                };
                Ok(HiveSnapshot {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    paired: cfg.node_key.is_some(),
                    config_path: nodeconfig::path().display().to_string(),
                    hub_url: cfg.hub_url.clone(),
                    llama_url: cfg.llama_url.clone(),
                    region: cfg.region.clone(),
                    model: model_pref(),
                    models: caps.models.iter().map(|m| m.id.clone()).collect(),
                    backend_ok,
                    running: self.worker_stop.lock().await.is_some(),
                    busy: *self.busy.lock().await,
                    pairing: self.pairing.lock().await.as_ref().map(|p| p.view.clone()),
                    summary_json,
                    activity: self.activity.lock().await.iter().cloned().collect(),
                    error: self.last_error.lock().await.take(),
                    worker_enabled: matches!(std::env::var("HIVE_WORKER_ENABLED").as_deref(), Ok("1")),
                    allow_internet: cfg.allow_internet,
                    tools_level: match cfg.tools_level {
                        ToolsLevel::InferenceOnly => "inference_only".to_string(),
                        ToolsLevel::SandboxedTools => "sandboxed_tools".to_string(),
                    },
                })
            })
            .await
            .map_err(|e| HiveError::Failed(format!("snapshot task panicked: {e}")))?
    }

    /// Only `HIVE_*` settings, never the node key itself (pairing/unpairing has its own path).
    pub fn set_config(&self, key: String, value: String) -> Result<String, HiveError> {
        if !key.starts_with("HIVE_") || key == "HIVE_NODE_KEY" {
            return Err(HiveError::Failed(
                "only HIVE_* settings (not the node key) can be changed here".into(),
            ));
        }
        let p = nodeconfig::set(&key, &value).map_err(HiveError::from)?;
        if value.trim().is_empty() {
            std::env::remove_var(&key);
        } else {
            std::env::set_var(&key, value.trim());
        }
        Ok(p.display().to_string())
    }

    pub async fn pair_begin(self: Arc<Self>) -> Result<PairingView, HiveError> {
        RUNTIME
            .spawn(async move {
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                if cfg.node_key.is_some() {
                    return Err(HiveError::Failed("this machine is already paired".into()));
                }
                if let Some(p) = self.pairing.lock().await.as_ref() {
                    return Ok(p.view.clone());
                }
                let hw = ohhive_core::probe::probe_hardware();
                let hint = serde_json::json!({
                    "hostname": hostname(),
                    "os": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                    "cpu": hw.cpu_model,
                    "gpu": hw.gpu_model,
                    "ram_gb": (hw.ram_bytes as f64 / 1e9).round(),
                    "app": "desktop-swift",
                });
                let p = Pairing::new(&cfg.hub_url, &cfg.anon_key);
                let start = p.begin(hint).await.map_err(HiveError::from)?;
                let view = PairingView {
                    code: start.code.clone(),
                    url: start.url.clone(),
                    expires_in_seconds: start.expires_in_seconds,
                };
                let (cancel_tx, mut cancel_rx) = watch::channel(false);
                *self.pairing.lock().await = Some(PairingHandle {
                    view: view.clone(),
                    cancel: cancel_tx,
                });
                self.log(
                    "info",
                    format!("pairing code {} \u{2014} waiting for ohghive.com/pair", start.code),
                )
                .await;

                let secret = start.secret.clone();
                let this = self.clone();
                RUNTIME.spawn(async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(3));
                    loop {
                        tokio::select! {
                            _ = tick.tick() => {}
                            _ = cancel_rx.changed() => { break; }
                        }
                        match p.poll(&secret).await {
                            Ok(PairingPoll::Pending) => continue,
                            Ok(PairingPoll::Expired) => {
                                this.log("error", "pairing code expired \u{2014} start again").await;
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
                                        // Seed local config with what was chosen on the pairing
                                        // page -- otherwise the first check-in would silently
                                        // reset both to their defaults.
                                        let allow_internet_s = if allow_internet { "true" } else { "false" };
                                        let _ = nodeconfig::set("HIVE_ALLOW_INTERNET", allow_internet_s);
                                        std::env::set_var("HIVE_ALLOW_INTERNET", allow_internet_s);
                                        let tools_level_s = match tools_level {
                                            ToolsLevel::InferenceOnly => "inference_only",
                                            ToolsLevel::SandboxedTools => "sandboxed_tools",
                                        };
                                        let _ = nodeconfig::set("HIVE_TOOLS_LEVEL", tools_level_s);
                                        std::env::set_var("HIVE_TOOLS_LEVEL", tools_level_s);
                                        this.log("ok", format!("paired as \u{201c}{display_name}\u{201d}")).await;
                                    }
                                    Err(e) => {
                                        this.log("error", format!("could not save node key: {e}")).await
                                    }
                                }
                                break;
                            }
                            Err(e) => {
                                this.log("error", format!("pairing poll failed: {e}")).await;
                                break;
                            }
                        }
                    }
                    *this.pairing.lock().await = None;
                    this.notify_changed().await;
                });
                Ok(view)
            })
            .await
            .map_err(|e| HiveError::Failed(format!("pair_begin task panicked: {e}")))?
    }

    pub async fn pair_cancel(&self) {
        if let Some(p) = self.pairing.lock().await.take() {
            let _ = p.cancel.send(true);
        }
    }

    pub async fn worker_start(self: Arc<Self>) -> Result<(), HiveError> {
        if self.worker_stop.lock().await.is_some() {
            return Ok(());
        }
        RUNTIME
            .spawn(async move {
                nodeconfig::export_env();
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let key = cfg
                    .node_key
                    .clone()
                    .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
                let (caps, ok) = capabilities(&cfg).await;
                if !ok || caps.models.is_empty() {
                    return Err(HiveError::Failed(format!(
                        "no models available at {} \u{2014} is Ollama running?",
                        cfg.llama_url
                    )));
                }
                let (stop_tx, stop_rx) = watch::channel(false);
                *self.worker_stop.lock().await = Some(stop_tx);
                let _ = nodeconfig::set("HIVE_WORKER_ENABLED", "1");
                std::env::set_var("HIVE_WORKER_ENABLED", "1");
                let model = model_pref();
                self.log(
                    "ok",
                    format!(
                        "working \u{2014} {} models, model {}",
                        caps.models.len(),
                        model.clone().unwrap_or_else(|| "auto".into())
                    ),
                )
                .await;

                let (events_tx, mut events_rx) = tokio::sync::broadcast::channel(64);
                let this = self.clone();
                RUNTIME.spawn(async move {
                    while let Ok(ev) = events_rx.recv().await {
                        if matches!(ev, WorkerEvent::Leased { .. }) {
                            *this.busy.lock().await = true;
                        }
                        if matches!(ev, WorkerEvent::Completed { .. } | WorkerEvent::Failed { .. } | WorkerEvent::Released { .. } | WorkerEvent::Idle) {
                            *this.busy.lock().await = false;
                        }
                        let (text, kind) = describe(&ev);
                        this.log(kind, text).await;
                    }
                });

                let this = self.clone();
                RUNTIME.spawn(async move {
                    let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
                    let be = LlamaCppBackend::new(&cfg.llama_url);
                    let r: anyhow::Result<()> = async {
                        hub.check_in(&caps, cfg.region.as_deref()).await?;
                        let sandbox = ohhive_core::sandbox::Sandbox::new()
                            .map_err(|e| anyhow::anyhow!("sandbox engine init failed: {e}"))?;
                        let w = Worker {
                            hub: &hub,
                            backend: &be,
                            caps: &caps,
                            default_model: model,
                            stop: stop_rx,
                            events: Some(events_tx),
                            data_dir: ohhive_core::sandbox::default_data_dir(),
                            sandbox: Some(&sandbox),
                        };
                        w.run_forever(std::time::Duration::from_secs(5), 6).await
                    }
                    .await;
                    if let Err(e) = r {
                        this.log("error", format!("worker stopped: {e}")).await;
                        let _ = hub.check_out().await;
                    } else {
                        this.log("info", "stopped \u{2014} checked out").await;
                    }
                    *this.worker_stop.lock().await = None;
                    *this.busy.lock().await = false;
                    this.notify_changed().await;
                });
                Ok(())
            })
            .await
            .map_err(|e| HiveError::Failed(format!("worker_start task panicked: {e}")))?
    }

    pub async fn worker_stop(&self, forget: bool) {
        if let Some(tx) = self.worker_stop.lock().await.as_ref() {
            let _ = tx.send(true);
            self.log("info", "stopping \u{2014} releasing any leased card").await;
        }
        if forget {
            let _ = nodeconfig::set("HIVE_WORKER_ENABLED", "0");
            std::env::set_var("HIVE_WORKER_ENABLED", "0");
        }
    }
}
