//! FFI wrapper for `hive_core::setup` (ADR-010, moved/shared per ADR-018 decision 2): hardware
//! assessment, Ollama install, model pull. Mirrors the Tauri app's `assess`/`ollama_install`/
//! `ollama_pull`/`setup_finish` commands (`apps/desktop/src-tauri/src/lib.rs`) one-for-one.
//!
//! `hive_core::capability::Hardware`/`GpuVendor` and `hive_core::setup::{Rung, OllamaState}`
//! aren't UniFFI types themselves (hive-core doesn't depend on uniffi -- it's linked into
//! `hive`/`hive-server`/`hive-coordinator` too, which have no reason to carry that dependency).
//! This module defines FFI-local mirrors and converts.

use crate::{HiveError, HiveNode, SetupProgress, RUNTIME};
use hive_core::capability::{GpuVendor, Hardware};
use hive_core::nodeconfig::{self, NodeConfig};
use hive_core::setup::{self, OllamaState, Rung};
use std::sync::Arc;

#[derive(uniffi::Enum, Clone, Copy)]
pub enum GpuVendorInfo {
    Apple,
    Nvidia,
    Amd,
    Intel,
    None,
}

impl From<GpuVendor> for GpuVendorInfo {
    fn from(v: GpuVendor) -> Self {
        match v {
            GpuVendor::Apple => Self::Apple,
            GpuVendor::Nvidia => Self::Nvidia,
            GpuVendor::Amd => Self::Amd,
            GpuVendor::Intel => Self::Intel,
            GpuVendor::None => Self::None,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct HardwareInfo {
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub ram_bytes: u64,
    pub gpu_vendor: GpuVendorInfo,
    pub gpu_model: Option<String>,
    pub vram_bytes: Option<u64>,
    pub disk_free_bytes: u64,
}

impl From<Hardware> for HardwareInfo {
    fn from(h: Hardware) -> Self {
        Self {
            cpu_model: h.cpu_model,
            cpu_cores: h.cpu_cores,
            ram_bytes: h.ram_bytes,
            gpu_vendor: h.gpu_vendor.into(),
            gpu_model: h.gpu_model,
            vram_bytes: h.vram_bytes,
            disk_free_bytes: h.disk_free_bytes,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct ModelRung {
    pub model: String,
    pub min_bytes: u64,
    pub download_bytes: u64,
    pub why: String,
}

impl From<Rung> for ModelRung {
    fn from(r: Rung) -> Self {
        Self {
            model: r.model,
            min_bytes: r.min_bytes,
            download_bytes: r.download_bytes,
            why: r.why,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct OllamaStateInfo {
    pub running: bool,
    pub version: Option<String>,
    pub installed_app: Option<String>,
    pub url: String,
}

impl From<OllamaState> for OllamaStateInfo {
    fn from(o: OllamaState) -> Self {
        Self {
            running: o.running,
            version: o.version,
            installed_app: o.installed_app,
            url: o.url,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct SetupAssessment {
    pub hardware: HardwareInfo,
    pub budget_bytes: u64,
    pub ollama: OllamaStateInfo,
    pub recommended: Option<ModelRung>,
    /// rungs that fit, best first
    pub fits: Vec<ModelRung>,
    /// models already present in Ollama
    pub present: Vec<String>,
    pub suggest_server: bool,
    pub os: String,
    pub arch: String,
}

/// Same hub RPC + fallback the Tauri app's `ladder()` uses (`apps/desktop/src-tauri/src/lib.rs`),
/// duplicated here rather than shared since it's five lines and pulling `HubClient` in just for
/// a plain REST POST isn't worth it.
async fn ladder(cfg: &NodeConfig) -> Vec<Rung> {
    let http = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return setup::builtin_ladder(),
    };
    let r = http
        .post(format!("{}/rest/v1/rpc/hive_model_ladder", cfg.hub_url))
        .header("apikey", &cfg.anon_key)
        .header("Authorization", format!("Bearer {}", cfg.anon_key))
        .json(&serde_json::json!({}))
        .send()
        .await;
    if let Ok(r) = r {
        if r.status().is_success() {
            if let Ok(v) = r.json::<Vec<Rung>>().await {
                if !v.is_empty() {
                    return v;
                }
            }
        }
    }
    setup::builtin_ladder()
}

#[uniffi::export]
impl HiveNode {
    /// Hardware + Ollama + model-ladder snapshot for the Setup flow. Safe to call repeatedly
    /// (e.g. after `ollamaInstall`/`ollamaPull` complete) -- it's a read, not a step.
    pub async fn assess(self: Arc<Self>) -> Result<SetupAssessment, HiveError> {
        RUNTIME
            .spawn(async move {
                nodeconfig::export_env();
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let l = ladder(&cfg).await;
                let a = setup::assess(&cfg.llama_url, &l).await;
                Ok(SetupAssessment {
                    hardware: a.hardware.into(),
                    budget_bytes: a.budget_bytes,
                    ollama: a.ollama.into(),
                    recommended: a.recommended.map(Into::into),
                    fits: a.fits.into_iter().map(Into::into).collect(),
                    present: a.present,
                    suggest_server: a.suggest_server,
                    os: a.os.to_string(),
                    arch: a.arch.to_string(),
                })
            })
            .await
            .map_err(|e| HiveError::Failed(format!("assess task panicked: {e}")))?
    }

    /// macOS-only automatic Ollama install (see `hive_core::setup::install_ollama`). Progress
    /// streams through `HiveEventListener.onSetupProgress`.
    pub async fn ollama_install(self: Arc<Self>) -> Result<(), HiveError> {
        {
            let mut b = self.setup_busy.lock().await;
            if *b {
                return Err(HiveError::Failed("setup step already running".into()));
            }
            *b = true;
        }
        self.log("info", "installing Ollama").await;
        let this = self.clone();
        let r = RUNTIME
            .spawn(async move {
                let reporter_node = this.clone();
                setup::install_ollama(move |p| {
                    let node = reporter_node.clone();
                    RUNTIME.spawn(async move {
                        node.emit_setup_progress(p.into()).await;
                    });
                })
                .await
            })
            .await
            .map_err(|e| HiveError::Failed(format!("ollama_install task panicked: {e}")))?;
        *self.setup_busy.lock().await = false;
        match r {
            Ok(()) => {
                self.log("ok", "Ollama installed and running").await;
                Ok(())
            }
            Err(e) => {
                self.emit_setup_progress(SetupProgress {
                    phase: "install".into(),
                    text: e.to_string(),
                    completed: 0,
                    total: 0,
                    done: true,
                    error: Some(e.to_string()),
                })
                .await;
                self.log("error", format!("Ollama install: {e}")).await;
                Err(HiveError::Failed(e.to_string()))
            }
        }
    }

    /// Pulls `model` via Ollama's streaming `/api/pull`, then makes it this machine's default
    /// (`HIVE_MODEL`). Progress streams through `HiveEventListener.onSetupProgress`.
    pub async fn ollama_pull(self: Arc<Self>, model: String) -> Result<(), HiveError> {
        {
            let mut b = self.setup_busy.lock().await;
            if *b {
                return Err(HiveError::Failed("setup step already running".into()));
            }
            *b = true;
        }
        nodeconfig::export_env();
        let cfg = match nodeconfig::load() {
            Ok(c) => c,
            Err(e) => {
                *self.setup_busy.lock().await = false;
                return Err(HiveError::from(e));
            }
        };
        self.log("info", format!("pulling {model}")).await;
        let this = self.clone();
        let model2 = model.clone();
        let r = RUNTIME
            .spawn(async move {
                let reporter_node = this.clone();
                setup::pull_model(&cfg.llama_url, &model2, move |p| {
                    let node = reporter_node.clone();
                    RUNTIME.spawn(async move {
                        node.emit_setup_progress(p.into()).await;
                    });
                })
                .await
            })
            .await
            .map_err(|e| HiveError::Failed(format!("ollama_pull task panicked: {e}")))?;
        *self.setup_busy.lock().await = false;
        match r {
            Ok(()) => {
                let _ = nodeconfig::set("HIVE_MODEL", &model);
                std::env::set_var("HIVE_MODEL", &model);
                self.log(
                    "ok",
                    format!("{model} ready \u{2014} it's now this machine's model"),
                )
                .await;
                Ok(())
            }
            Err(e) => {
                self.emit_setup_progress(SetupProgress {
                    phase: "pull".into(),
                    text: e.to_string(),
                    completed: 0,
                    total: 0,
                    done: true,
                    error: Some(e.to_string()),
                })
                .await;
                self.log("error", format!("pull {model}: {e}")).await;
                Err(HiveError::Failed(e.to_string()))
            }
        }
    }

    /// Marks first-run Setup complete (`HIVE_SETUP_DONE=1`) so `snapshot().setupDone` flips and
    /// the Swift UI can stop offering the Setup flow.
    pub fn setup_finish(&self) -> Result<(), HiveError> {
        nodeconfig::set("HIVE_SETUP_DONE", "1").map_err(HiveError::from)?;
        std::env::set_var("HIVE_SETUP_DONE", "1");
        Ok(())
    }
}

impl From<setup::Progress> for SetupProgress {
    fn from(p: setup::Progress) -> Self {
        Self {
            phase: p.phase,
            text: p.text,
            completed: p.completed,
            total: p.total,
            done: p.done,
            error: p.error,
        }
    }
}
