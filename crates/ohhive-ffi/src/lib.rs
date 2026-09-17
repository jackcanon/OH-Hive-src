//! hive-ffi -- UniFFI bridge exposing hive-core's phase-1 desktop surface (ADR-018) to the
//! native macOS SwiftUI app: pairing, the compute-node worker, snapshot/about, config, and an
//! activity/change event callback. This mirrors `apps/desktop/src-tauri/src/lib.rs`'s Tauri IPC
//! surface one-for-one for everything phase 1 covers.
//!
//! Phase 2 (ADR-018 decision 4) adds the first-run Setup wizard (`setup.rs` in this crate,
//! wrapping `hive_core::setup`) as a second `#[uniffi::export] impl HiveNode` block. Still not
//! wrapped: the regional-server role and Cloudflare Tunnel (`tunnel.rs` in the Tauri app).
//!
//! No hand-maintained `.udl` file: every exported type/fn/method is declared with proc-macro
//! attributes right here, and `src/bin/uniffi-bindgen.rs` generates the Swift binding module
//! from this crate directly.

use hive_core::backend::llama_cpp::LlamaCppBackend;
use hive_core::backend::Backend;
use hive_core::capability::{Capabilities, Modality, ToolsLevel};
use hive_core::hub::{HubClient, HubError, Pairing, PairingPoll};
use hive_core::nodeconfig::{self, NodeConfig};
use hive_core::supervisor::{Intent, SupervisedWorker, Supervisor, WorkerStatus};
use hive_core::worker::{Worker, WorkerEvent};
use once_cell::sync::Lazy;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{watch, Mutex as AsyncMutex};

mod bots;
mod bots_storage;
mod byok_keys;
mod channel;
mod chat;
mod feedback;
mod kanban;
mod local_hub;
mod media;
mod private_fleet;
mod release_notes;
mod repository_projects;
mod server;
mod setup;
mod skills;
mod subscription;
mod tunnel;

uniffi::setup_scaffolding!();

/// One shared multi-threaded Tokio runtime for the whole FFI surface. Every exported async
/// method spawns its real work onto this runtime and awaits the JoinHandle, instead of letting
/// UniFFI's own foreign-future bridge poll tokio/reqwest internals directly -- `HubClient` and
/// `Pairing` use `reqwest`, which needs a live Tokio reactor under whatever task calls it, and
/// there's no guarantee UniFFI's async bridge provides one. Swift never sees any of this; it
/// just gets ordinary Swift `async` functions.
pub(crate) static RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
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
    /// M8 media backends -- `None` when not configured (Settings > Media backends). Unlike
    /// `llama_url`, these have no default: most Macs don't run a whisper.cpp server or ComfyUI
    /// instance, so this being empty is the normal, expected state until the owner sets one up.
    pub whisper_url: Option<String>,
    pub whisper_model: Option<String>,
    pub comfyui_url: Option<String>,
    pub comfyui_checkpoint: Option<String>,
    pub region: Option<String>,
    pub model: Option<String>,
    pub models: Vec<String>,
    pub backend_ok: bool,
    /// Taking work right now. Kept for the existing toggle, which only ever asked on/off.
    pub running: bool,
    /// One of `resting`, `working`, `retrying`, `blocked`.
    ///
    /// `running` cannot express the two that matter when something has gone wrong: a node retrying
    /// after an end nobody asked for, and one stopped for a cause a restart cannot fix. Both used
    /// to render as "not working" -- identical to the member having switched it off, which is most
    /// of how Midgaard sat idle for twelve hours without anyone noticing.
    pub worker_state: String,
    /// Why, for the two states that have a reason: "hub unreachable, retrying in 60s", "pair this
    /// machine first". `None` while resting or working, where there is nothing to explain.
    pub worker_detail: Option<String>,
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
    /// Mirrors the Tauri app's `setup_done` -- once true, the Swift UI can stop offering the
    /// first-run Setup flow (`hive-ffi::setup`) and default straight to the Node view.
    pub setup_done: bool,
    pub private_fleet_enrolled: bool,
}

#[derive(uniffi::Record, Clone)]
pub struct SetupProgress {
    pub phase: String,
    pub text: String,
    pub completed: u64,
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
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

impl From<hive_core::skills::SkillError> for HiveError {
    fn from(e: hive_core::skills::SkillError) -> Self {
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
    /// Fired during `HiveNode.ollamaInstall()`/`ollamaPull()` (phase 2, `setup.rs`). Mirrors the
    /// Tauri app's `setup` window event.
    fn on_setup_progress(&self, progress: SetupProgress);
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

fn env_flag(k: &str) -> bool {
    matches!(
        std::env::var(k).as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

fn model_pref() -> Option<String> {
    std::env::var("HIVE_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn capabilities(cfg: &NodeConfig) -> (Capabilities, bool) {
    let hardware = hive_core::probe::probe_hardware();
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
    hive_core::model_fit::filter_models(&hardware, &mut models);
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
            acceptance: hive_core::capability::Capabilities::RUNS_ACCEPTANCE,
        },
        ok,
    )
}

/// One human-readable line + activity kind per `WorkerEvent`, same idea as the Tauri app's
/// `describe()`.
fn describe(e: &WorkerEvent) -> (String, &'static str) {
    match e {
        WorkerEvent::Leased {
            card,
            project,
            resume,
        } => (
            format!(
                "{} \u{201c}{card}\u{201d} for {project}",
                if *resume { "resuming" } else { "leased" }
            ),
            "info",
        ),
        WorkerEvent::Step { card, step, .. } => (format!("{card}: step {step}"), "info"),
        WorkerEvent::Completed {
            card,
            project,
            earned_honey,
            ..
        } => (
            format!(
                "finished \u{201c}{card}\u{201d} for {project} \u{2014} +{earned_honey:.4} honey"
            ),
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
    /// What the member asked for, and the authority on whether this node should be working.
    ///
    /// Replaces the old `worker_stop: Option<Sender<bool>>`, which was a handle to a *running
    /// task* and therefore answered a different question than the one the UI was asking. Present
    /// once the supervisor is running; the supervisor outlives any individual worker run.
    intent: AsyncMutex<Option<watch::Sender<Intent>>>,
    /// What the supervisor is actually doing, for `snapshot`.
    worker_status: AsyncMutex<Option<watch::Receiver<WorkerStatus>>>,
    busy: AsyncMutex<bool>,
    activity: AsyncMutex<VecDeque<ActivityEntry>>,
    pairing: AsyncMutex<Option<PairingHandle>>,
    last_error: AsyncMutex<Option<String>>,
    listener: AsyncMutex<Option<Box<dyn HiveEventListener>>>,
    /// Guards against overlapping `assess`/`ollamaInstall`/`ollamaPull` calls (`setup.rs`),
    /// matching the Tauri app's `AppState.setup_busy`.
    setup_busy: AsyncMutex<bool>,
    /// Regional-server role (`server.rs`), matching the Tauri app's `AppState.server_stop` /
    /// `server_status`. `None` = not running. Tunnel lifecycle (ADR-018 task #71) isn't wired up
    /// yet -- this phase only supports a manually-configured public URL, same as the Tauri app's
    /// `tn.available == false` path in `Server.tsx`.
    pub(crate) server_stop: AsyncMutex<Option<watch::Sender<bool>>>,
    pub(crate) server_status: Arc<hive_server::ServerStatus>,
    /// `cloudflared tunnel run`, alive exactly while the regional server role is on and a tunnel
    /// is configured (`tunnel.rs`). Owned here so `server_stop` can kill it alongside
    /// hive-server, matching the Tauri app's `AppState.tunnel_child`.
    pub(crate) tunnel_child: AsyncMutex<Option<tokio::process::Child>>,
    /// Private Fleet vault (ADR-028, `local_hub.rs`). Its own `std::sync::Mutex`-backed state,
    /// not `AsyncMutex` -- every vault operation is local SQLite, no network, so it never needs
    /// to hold a lock across an `.await`.
    pub(crate) vault: local_hub::VaultState,
    fleet: private_fleet::FleetState,
}

impl HiveNode {
    pub(crate) async fn log(&self, kind: &str, text: impl Into<String>) {
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

    pub(crate) async fn notify_changed(&self) {
        if let Some(l) = self.listener.lock().await.as_ref() {
            l.on_changed();
        }
    }

    pub(crate) async fn emit_setup_progress(&self, progress: SetupProgress) {
        if let Some(l) = self.listener.lock().await.as_ref() {
            l.on_setup_progress(progress);
        }
    }
}

// clippy::new_without_default wants this since `new()` takes no arguments -- kept as a plain
// trait impl (not inside the #[uniffi::export] block below) so it doesn't affect the FFI surface.
impl Default for HiveNode {
    fn default() -> Self {
        Self::new()
    }
}

#[uniffi::export]
impl HiveNode {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            intent: AsyncMutex::new(None),
            worker_status: AsyncMutex::new(None),
            busy: AsyncMutex::new(false),
            activity: AsyncMutex::new(VecDeque::new()),
            pairing: AsyncMutex::new(None),
            last_error: AsyncMutex::new(None),
            listener: AsyncMutex::new(None),
            setup_busy: AsyncMutex::new(false),
            server_stop: AsyncMutex::new(None),
            server_status: Arc::new(hive_server::ServerStatus::default()),
            tunnel_child: AsyncMutex::new(None),
            vault: local_hub::VaultState::new(),
            fleet: private_fleet::FleetState::default(),
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
            core_version: hive_core::VERSION.to_string(),
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
                let status = self.worker_state().await;
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
                let node = self.clone();
                let private_fleet_enrolled = RUNTIME
                    .spawn_blocking(move || node.private_fleet_is_enrolled())
                    .await
                    .map_err(|_| {
                        HiveError::Failed("Cannot check Private Fleet identity".into())
                    })??;
                Ok(HiveSnapshot {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    paired: cfg.node_key.is_some(),
                    config_path: nodeconfig::path().display().to_string(),
                    hub_url: cfg.hub_url.clone(),
                    llama_url: cfg.llama_url.clone(),
                    whisper_url: cfg.whisper_url.clone(),
                    whisper_model: cfg.whisper_model.clone(),
                    comfyui_url: cfg.comfyui_url.clone(),
                    comfyui_checkpoint: cfg.comfyui_checkpoint.clone(),
                    region: cfg.region.clone(),
                    model: model_pref(),
                    models: caps.models.iter().map(|m| m.id.clone()).collect(),
                    backend_ok,
                    running: matches!(status, WorkerStatus::Working),
                    worker_state: state_name(&status).into(),
                    worker_detail: state_detail(&status),
                    busy: *self.busy.lock().await,
                    pairing: self.pairing.lock().await.as_ref().map(|p| p.view.clone()),
                    summary_json,
                    activity: self.activity.lock().await.iter().cloned().collect(),
                    error: self.last_error.lock().await.take(),
                    worker_enabled: matches!(
                        std::env::var("HIVE_WORKER_ENABLED").as_deref(),
                        Ok("1")
                    ),
                    allow_internet: cfg.allow_internet,
                    tools_level: match cfg.tools_level {
                        ToolsLevel::InferenceOnly => "inference_only".to_string(),
                        ToolsLevel::SandboxedTools => "sandboxed_tools".to_string(),
                    },
                    setup_done: (cfg.node_key.is_some() || private_fleet_enrolled)
                        && env_flag("HIVE_SETUP_DONE"),
                    private_fleet_enrolled,
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
                let hw = hive_core::probe::probe_hardware();
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
                    format!(
                        "pairing code {} \u{2014} waiting for ohghive.com/pair",
                        start.code
                    ),
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
                                this.log("error", "pairing code expired \u{2014} start again")
                                    .await;
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
                                        let allow_internet_s =
                                            if allow_internet { "true" } else { "false" };
                                        let _ = nodeconfig::set(
                                            "HIVE_ALLOW_INTERNET",
                                            allow_internet_s,
                                        );
                                        std::env::set_var("HIVE_ALLOW_INTERNET", allow_internet_s);
                                        let tools_level_s = match tools_level {
                                            ToolsLevel::InferenceOnly => "inference_only",
                                            ToolsLevel::SandboxedTools => "sandboxed_tools",
                                        };
                                        let _ = nodeconfig::set("HIVE_TOOLS_LEVEL", tools_level_s);
                                        std::env::set_var("HIVE_TOOLS_LEVEL", tools_level_s);
                                        this.log(
                                            "ok",
                                            format!("paired as \u{201c}{display_name}\u{201d}"),
                                        )
                                        .await;
                                    }
                                    Err(e) => {
                                        this.log("error", format!("could not save node key: {e}"))
                                            .await
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

    /// Start the supervisor if it is not already running, and tell it the member wants work.
    ///
    /// Before 2026-09-17 this spawned a worker task and forgot about it: nothing owned the loop
    /// afterwards, so any end at all -- asked for or not -- retired the node until a human noticed
    /// and flipped a toggle. Now the only thing this changes is intent; `hive_core::supervisor`
    /// owns the lifecycle. See that module's header for what went wrong and why.
    pub async fn worker_start(self: Arc<Self>) -> Result<(), HiveError> {
        // Kept deliberately: the supervisor would surface both of these as `Blocked`, but a member
        // who just pressed a button deserves the answer now rather than one poll later. Checking
        // here also means a node that cannot possibly work does not get "work" persisted as its
        // intent, so it will not try again on every launch.
        let cfg = RUNTIME
            .spawn(async {
                nodeconfig::export_env();
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                if cfg.node_key.is_none() {
                    return Err(HiveError::Failed("pair this machine first".into()));
                }
                let (caps, ok) = capabilities(&cfg).await;
                if !ok || caps.models.is_empty() {
                    return Err(HiveError::Failed(format!(
                        "no models available at {} \u{2014} is Ollama running?",
                        cfg.llama_url
                    )));
                }
                Ok(cfg)
            })
            .await
            .map_err(|e| HiveError::Failed(format!("worker_start task panicked: {e}")))??;
        let _ = nodeconfig::set("HIVE_WORKER_ENABLED", "1");
        std::env::set_var("HIVE_WORKER_ENABLED", "1");
        drop(cfg);
        self.supervise_with(Intent::Work).await;
        Ok(())
    }

    /// Tell the supervisor the member does not want work.
    ///
    /// `forget` is the difference between "stop for now" and "stop, and do not start yourself at
    /// the next launch". Only the UI toggle passes true.
    pub async fn worker_stop(&self, forget: bool) {
        if forget {
            let _ = nodeconfig::set("HIVE_WORKER_ENABLED", "0");
            std::env::set_var("HIVE_WORKER_ENABLED", "0");
        }
        if let Some(tx) = self.intent.lock().await.as_ref() {
            let _ = tx.send(Intent::Rest);
            self.log("info", "stopping \u{2014} releasing any leased card")
                .await;
        }
    }

    /// Start the supervisor with whatever the member last chose, and let it act on that.
    ///
    /// Swift calls this once at launch. It is the whole of the resume-at-login behaviour the Tauri
    /// app has had since `apps/desktop/src-tauri/src/lib.rs:1043` ("resume roles the member had
    /// on") and the Swift app never got -- which is why a node that stopped stayed stopped.
    ///
    /// Safe to call more than once; the second call does nothing.
    pub async fn supervise(self: Arc<Self>) {
        nodeconfig::export_env();
        let intent = if env_flag("HIVE_WORKER_ENABLED") {
            Intent::Work
        } else {
            Intent::Rest
        };
        self.supervise_with(intent).await;
    }
}

impl HiveNode {
    /// Idempotently start the supervisor, then set intent. Split out because both `worker_start`
    /// and `supervise` need exactly this and the ordering matters: the supervisor has to exist
    /// before there is anywhere to send intent.
    async fn supervise_with(self: &Arc<Self>, intent: Intent) {
        let mut held = self.intent.lock().await;
        if let Some(tx) = held.as_ref() {
            let _ = tx.send(intent);
            return;
        }
        let (tx, rx) = watch::channel(intent);
        let worker = Arc::new(NodeWorker {
            node: Arc::downgrade(self),
        });
        let (supervisor, status) = Supervisor::new(worker, rx);
        *self.worker_status.lock().await = Some(status);
        *held = Some(tx);
        drop(held);
        let node = self.clone();
        RUNTIME.spawn(async move {
            // Repaint on every transition so the UI shows "retrying in 60s" the moment it becomes
            // true rather than at the next five-second poll.
            let mut watched = node.worker_state_receiver().await;
            RUNTIME.spawn(async move {
                while let Some(rx) = watched.as_mut() {
                    if rx.changed().await.is_err() {
                        break;
                    }
                    node.notify_changed().await;
                }
            });
        });
        RUNTIME.spawn(supervisor.run());
    }

    async fn worker_state_receiver(&self) -> Option<watch::Receiver<WorkerStatus>> {
        self.worker_status.lock().await.clone()
    }

    /// The supervisor's current state, or `Resting` before it has been started.
    pub(crate) async fn worker_state(&self) -> WorkerStatus {
        match self.worker_status.lock().await.as_ref() {
            Some(rx) => rx.borrow().clone(),
            None => WorkerStatus::Resting,
        }
    }
}

fn state_name(status: &WorkerStatus) -> &'static str {
    match status {
        WorkerStatus::Resting => "resting",
        WorkerStatus::Working => "working",
        WorkerStatus::Retrying { .. } => "retrying",
        WorkerStatus::Blocked { .. } => "blocked",
    }
}

fn state_detail(status: &WorkerStatus) -> Option<String> {
    match status {
        WorkerStatus::Resting | WorkerStatus::Working => None,
        WorkerStatus::Retrying {
            reason,
            next_attempt_in,
            ..
        } => Some(format!(
            "{reason}; retrying in {}s",
            next_attempt_in.as_secs()
        )),
        WorkerStatus::Blocked { reason } => Some(reason.clone()),
    }
}

/// The worker, as the supervisor drives it.
///
/// Everything is rebuilt per run on purpose: a restart re-reads config and re-probes the backend,
/// so a node that was blocked because Ollama was down starts working when Ollama comes back,
/// without the member touching anything.
struct NodeWorker {
    /// Weak, not strong: the supervisor task outlives any single run and a strong reference here
    /// would be a cycle through `HiveNode`, keeping it alive after Swift has let go.
    node: std::sync::Weak<HiveNode>,
}

impl NodeWorker {
    async fn log(&self, kind: &str, text: impl Into<String>) {
        if let Some(node) = self.node.upgrade() {
            node.log(kind, text).await;
        }
    }
}

#[async_trait::async_trait]
impl SupervisedWorker for NodeWorker {
    async fn run(
        &self,
        stop: watch::Receiver<bool>,
    ) -> anyhow::Result<hive_core::worker::WorkerExit> {
        nodeconfig::export_env();
        let cfg = nodeconfig::load()?;
        let key = cfg
            .node_key
            .clone()
            .ok_or_else(|| anyhow::anyhow!("pair this machine first"))?;
        let (caps, ok) = capabilities(&cfg).await;
        if !ok || caps.models.is_empty() {
            anyhow::bail!(
                "no models available at {} \u{2014} is Ollama running?",
                cfg.llama_url
            );
        }
        let model = model_pref();
        // Per RUN, not per start: a restart the member never asked for has to be visible in the
        // activity feed, or the supervisor is just hiding the problem more politely than before.
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
        if let Some(node) = self.node.upgrade() {
            // Ends on its own when the worker drops the sender at the end of this run.
            RUNTIME.spawn(async move {
                while let Ok(ev) = events_rx.recv().await {
                    if matches!(ev, WorkerEvent::Leased { .. }) {
                        *node.busy.lock().await = true;
                    }
                    if matches!(
                        ev,
                        WorkerEvent::Completed { .. }
                            | WorkerEvent::Failed { .. }
                            | WorkerEvent::Released { .. }
                            | WorkerEvent::Idle
                    ) {
                        *node.busy.lock().await = false;
                    }
                    let (text, kind) = describe(&ev);
                    node.log(kind, text).await;
                }
                *node.busy.lock().await = false;
            });
        }

        let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
        let be = LlamaCppBackend::new(&cfg.llama_url);
        hub.check_in(&caps, cfg.region.as_deref()).await?;
        let sandbox = hive_core::sandbox::Sandbox::new()
            .map_err(|e| anyhow::anyhow!("sandbox engine init failed: {e}"))?;
        let w = Worker {
            hub: &hub,
            backend: &be,
            caps: &caps,
            default_model: model,
            stop,
            events: Some(events_tx),
            data_dir: hive_core::sandbox::default_data_dir(),
            sandbox: Some(&sandbox),
        };
        w.run_until_stopped(std::time::Duration::from_secs(5), 6)
            .await
    }

    /// Say this machine is alive while it is not taking work.
    ///
    /// Lands in `hive.nodes.last_seen` and never in `last_heartbeat` -- migration 20260917150000
    /// keeps those apart so the orphaned-lease reaper still reads availability. Without this a
    /// checked-out machine is indistinguishable from an unplugged one.
    async fn ping(&self) {
        let Ok(cfg) = nodeconfig::load() else { return };
        let Some(key) = cfg.node_key.clone() else {
            return;
        };
        let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
        if let Err(e) = hub.heartbeat(None).await {
            // Ordinary. The next one is a minute away and the fleet view reads the last success.
            tracing::debug!("idle heartbeat failed: {e}");
        }
    }

    fn is_terminal(&self, error: &anyhow::Error) -> bool {
        let text = error.to_string();
        // Three things a restart genuinely cannot fix. Everything else -- the hub having a bad
        // minute, a dropped connection, a sandbox that failed to start once -- is worth retrying,
        // because the failure nobody has seen yet is more often transient than permanent.
        text.contains("pair this machine first")
            || text.contains("no models available")
            || text.contains("invalid_or_revoked_node_key")
    }
}
