//! First-run setup: turn a vanilla machine into a productive Hive member with as little typing
//! as possible (ADR-010). Assess the hardware, get Ollama onto the box, pick the most capable
//! model that fits, pull it with progress, then hand off to pairing.
//!
//! Model choice is a "ladder" keyed by usable accelerator memory (VRAM, or ~75 % of unified
//! memory on Apple Silicon — see `probe_gpu`). The hub can override the ladder through
//! `hive.settings.model_ladder` so the recommendation improves without shipping a new app.
//!
//! Moved here from `apps/desktop/src-tauri/src/setup.rs` (ADR-018 decision 2): both the Tauri
//! shell (Windows/Linux) and the native Swift shell (macOS, via `hive-ffi`) call this same
//! logic rather than each having their own copy.

use crate::capability::Hardware;
use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Rung {
    /// Ollama tag
    pub model: String,
    /// bytes of accelerator memory needed (weights + a working context)
    pub min_bytes: u64,
    /// approximate download size, bytes
    pub download_bytes: u64,
    pub why: String,
}

/// Built-in ladder, best first. Sizes are Ollama's Q4-ish defaults plus headroom.
pub fn builtin_ladder() -> Vec<Rung> {
    let gb = 1u64 << 30;
    vec![
        Rung {
            model: "qwen3.6:27b".into(),
            min_bytes: 22 * gb,
            download_bytes: 17 * gb,
            why: "strongest general model the Hive uses; needs ~22 GB".into(),
        },
        Rung {
            model: "gemma4:12b-it-qat".into(),
            min_bytes: 11 * gb,
            download_bytes: 8 * gb,
            why: "the Hive's interview model — fast, capable, fits in 12 GB".into(),
        },
        Rung {
            model: "gemma3:4b".into(),
            min_bytes: 5 * gb,
            download_bytes: 33 * gb / 10,
            why: "small but real; good for short text cards".into(),
        },
        Rung {
            model: "gemma3:1b".into(),
            min_bytes: 2 * gb,
            download_bytes: gb,
            why: "tiny — keeps a low-memory machine useful for simple cards".into(),
        },
    ]
}

#[derive(Serialize, Clone, Debug)]
pub struct Assessment {
    pub hardware: Hardware,
    /// bytes of accelerator memory we plan against
    pub budget_bytes: u64,
    pub ollama: OllamaState,
    pub recommended: Option<Rung>,
    /// rungs that fit, best first
    pub fits: Vec<Rung>,
    /// models already present in Ollama
    pub present: Vec<String>,
    /// enough free disk to also be a regional server (ADR-013 §G asks for 2–4 TB; we suggest at 250 GB)
    pub suggest_server: bool,
    pub os: &'static str,
    pub arch: &'static str,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct OllamaState {
    pub running: bool,
    pub version: Option<String>,
    pub installed_app: Option<String>,
    pub url: String,
}

pub async fn ollama_state(url: &str) -> OllamaState {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap();
    let version = http
        .get(format!("{}/api/version", url.trim_end_matches('/')))
        .send()
        .await
        .ok()
        .filter(|r| r.status().is_success());
    let version = match version {
        Some(r) => r
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.get("version").and_then(|s| s.as_str()).map(String::from)),
        None => None,
    };
    OllamaState {
        running: version.is_some(),
        version,
        installed_app: find_ollama_app().map(|p| p.display().to_string()),
        url: url.to_string(),
    }
}

pub fn find_ollama_app() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("/Applications/Ollama.app")];
    if let Some(h) = dirs::home_dir() {
        candidates.push(h.join("Applications").join("Ollama.app"));
    }
    candidates.into_iter().find(|p| p.exists()).or_else(|| {
        [
            "/opt/homebrew/bin/ollama",
            "/usr/local/bin/ollama",
            "/usr/bin/ollama",
        ]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
    })
}

pub async fn present_models(url: &str) -> Vec<String> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap();
    let Ok(r) = http
        .get(format!("{}/api/tags", url.trim_end_matches('/')))
        .send()
        .await
    else {
        return vec![];
    };
    let Ok(v) = r.json::<serde_json::Value>().await else {
        return vec![];
    };
    v.get("models")
        .and_then(|m| m.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

pub fn budget(hw: &Hardware) -> u64 {
    hw.vram_bytes.unwrap_or(hw.ram_bytes / 2)
}

pub async fn assess(llama_url: &str, ladder: &[Rung]) -> Assessment {
    let hardware = crate::probe::probe_hardware();
    let budget_bytes = budget(&hardware);
    let ollama = ollama_state(llama_url).await;
    let present = if ollama.running {
        present_models(llama_url).await
    } else {
        vec![]
    };
    let fits: Vec<Rung> = ladder
        .iter()
        .filter(|r| {
            r.min_bytes <= budget_bytes
                && r.download_bytes + (2u64 << 30) < hardware.disk_free_bytes
        })
        .cloned()
        .collect();
    Assessment {
        recommended: fits.first().cloned(),
        fits,
        present,
        suggest_server: hardware.disk_free_bytes >= 250 * (1u64 << 30),
        budget_bytes,
        hardware,
        ollama,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Progress {
    pub phase: String,
    pub text: String,
    pub completed: u64,
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
}

/// macOS: download the official Ollama.app zip, unpack to ~/Applications, launch it (it starts the
/// server and runs at login on its own). Reports through `report`. Other OSes: tell the caller to
/// use the official installer.
pub async fn install_ollama(report: impl Fn(Progress) + Send + Sync + 'static) -> Result<()> {
    if std::env::consts::OS != "macos" {
        anyhow::bail!("automatic install is macOS-only for now — run `curl -fsSL https://ollama.com/install.sh | sh` (Linux) or install from ollama.com (Windows), then come back");
    }
    let apps = dirs::home_dir()
        .context("no home dir")?
        .join("Applications");
    std::fs::create_dir_all(&apps)?;
    let zip = std::env::temp_dir().join("Ollama-darwin.zip");
    report(Progress {
        phase: "download".into(),
        text: "downloading Ollama".into(),
        completed: 0,
        total: 0,
        done: false,
        error: None,
    });
    let http = reqwest::Client::builder().build()?;
    let resp = http
        .get("https://ollama.com/download/Ollama-darwin.zip")
        .send()
        .await?
        .error_for_status()?;
    let total = resp.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(&zip).await?;
    let mut stream = resp.bytes_stream();
    let mut got = 0u64;
    let mut last = 0u64;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        got += chunk.len() as u64;
        if got - last > 4 << 20 {
            last = got;
            report(Progress {
                phase: "download".into(),
                text: "downloading Ollama".into(),
                completed: got,
                total,
                done: false,
                error: None,
            });
        }
    }
    file.flush().await?;
    drop(file);
    report(Progress {
        phase: "unpack".into(),
        text: "installing Ollama.app".into(),
        completed: got,
        total,
        done: false,
        error: None,
    });
    let out = tokio::process::Command::new("ditto")
        .args(["-x", "-k", zip.to_str().unwrap(), apps.to_str().unwrap()])
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!("unpack failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let app = apps.join("Ollama.app");
    if !app.exists() {
        anyhow::bail!("Ollama.app did not appear in {}", apps.display());
    }
    report(Progress {
        phase: "launch".into(),
        text: "starting Ollama".into(),
        completed: 0,
        total: 0,
        done: false,
        error: None,
    });
    tokio::process::Command::new("open")
        .args(["-g", "-a", app.to_str().unwrap()])
        .output()
        .await?;
    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if ollama_state("http://127.0.0.1:11434").await.running {
            report(Progress {
                phase: "launch".into(),
                text: "Ollama is running".into(),
                completed: 1,
                total: 1,
                done: true,
                error: None,
            });
            return Ok(());
        }
    }
    anyhow::bail!("Ollama installed but its server hasn't answered yet — open Ollama from Applications, then retry")
}

#[derive(Deserialize)]
struct PullLine {
    status: Option<String>,
    completed: Option<u64>,
    total: Option<u64>,
    error: Option<String>,
}

/// `POST /api/pull` streaming NDJSON, forwarded as progress.
pub async fn pull_model(
    url: &str,
    model: &str,
    report: impl Fn(Progress) + Send + Sync + 'static,
) -> Result<()> {
    let http = reqwest::Client::builder().build()?;
    let resp = http
        .post(format!("{}/api/pull", url.trim_end_matches('/')))
        .json(&serde_json::json!({ "name": model, "stream": true }))
        .send()
        .await?
        .error_for_status()?;
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::new();
    let mut last_report = std::time::Instant::now();
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk?);
        while let Some(i) = buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buf.drain(..=i).collect();
            let Ok(p) = serde_json::from_slice::<PullLine>(&line) else {
                continue;
            };
            if let Some(e) = p.error {
                anyhow::bail!("pull failed: {e}");
            }
            let status = p.status.unwrap_or_default();
            let done = status == "success";
            if done || last_report.elapsed().as_millis() > 400 {
                last_report = std::time::Instant::now();
                report(Progress {
                    phase: "pull".into(),
                    text: format!("{model}: {status}"),
                    completed: p.completed.unwrap_or(0),
                    total: p.total.unwrap_or(0),
                    done,
                    error: None,
                });
            }
            if done {
                return Ok(());
            }
        }
    }
    Ok(())
}
