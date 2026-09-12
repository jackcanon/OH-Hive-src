//! `hive` — OH Hive node CLI (ADR-003 D66). Subcommands map to what the
//! desktop app does in its GUI (ADR-010), so a headless Linux box can be a
//! compute node without Tauri.

mod config;
mod worker;

use anyhow::Result;
use chrono::{Datelike, Timelike};
use clap::{Parser, Subcommand};
use hive_core::backend::{mock::MockBackend, Backend};
use hive_core::capability::{Capabilities, Modality, Requirements, ToolsLevel};
use hive_core::hub::HubClient;
use hive_core::job::{Job, JobKind};

#[derive(Parser)]
#[command(name = "hive", version = hive_core::VERSION, about = "OH Hive node")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Probe hardware + backends and print the capabilities this node would advertise.
    Probe,
    /// Run one prompt through a backend locally (no coordinator).
    ///
    /// For `whisper`, `prompt` is ignored and `--audio-path` is required instead.
    /// For `comfyui`, `prompt` is the image prompt (`--negative-prompt` optional).
    Run {
        #[arg(default_value = "")]
        prompt: String,
        /// `mock`, `llama_cpp`, `whisper`, or `comfyui`.
        #[arg(long, default_value = "llama_cpp")]
        backend: String,
        #[arg(long, env = "HIVE_MODEL")]
        model: Option<String>,
        #[arg(long, default_value_t = 256)]
        max_tokens: u64,
        /// Local audio file to transcribe (`--backend whisper`).
        #[arg(long)]
        audio_path: Option<String>,
        /// BCP-47-ish language hint for whisper, e.g. "en" (omit to auto-detect).
        #[arg(long)]
        language: Option<String>,
        /// Negative prompt for `--backend comfyui`.
        #[arg(long, default_value = "")]
        negative_prompt: String,
    },
    /// List models the llama_cpp backend can see.
    Models,
    /// Show this node as the hub sees it.
    Status,
    /// Publish capabilities and become eligible for work.
    CheckIn {
        /// Keep running and heartbeat every N seconds (Ctrl-C checks out).
        #[arg(long)]
        stay: bool,
        #[arg(long, default_value_t = 30)]
        interval: u64,
    },
    /// Stop accepting work (drains if a lease is held).
    CheckOut,
    /// Check in and work cards until Ctrl-C: heartbeat, claim, run, report.
    Work {
        /// Seconds between dispatch polls.
        #[arg(long, default_value_t = 5)]
        poll: u64,
        /// Fallback model when a card doesn't require one.
        #[arg(long, env = "HIVE_MODEL")]
        model: Option<String>,
    },
    /// Write a config value to ~/.config/ohhive/node.env (e.g. `hive set HIVE_REGION us-west`).
    Set { key: String, value: String },
    /// Pair this machine with your Hive account (prints a code to enter on ohghive.com).
    Pair,
}

async fn capabilities(cfg: &config::NodeConfig) -> Result<Capabilities> {
    let hardware = hive_core::probe::probe_hardware();
    let mut modalities = vec![];
    let mut models = vec![];
    #[cfg(feature = "llama-cpp")]
    {
        let be = hive_core::backend::llama_cpp::LlamaCppBackend::new(&cfg.llama_url);
        match be.capabilities().await {
            Ok(c) => {
                modalities.extend(c.modalities);
                models.extend(c.models);
            }
            Err(e) => tracing::warn!("llama_cpp backend at {} unavailable: {e}", cfg.llama_url),
        }
    }
    // M8: only probed when configured (see nodeconfig.rs doc comment) — most nodes
    // don't run a whisper.cpp server or ComfyUI instance, so an unconfigured node
    // shouldn't eat a failed-connection warning on every heartbeat.
    #[cfg(feature = "whisper")]
    if let Some(url) = &cfg.whisper_url {
        let be = hive_core::backend::whisper::WhisperCppBackend::new(
            url,
            cfg.whisper_model
                .clone()
                .unwrap_or_else(|| "unknown".into()),
        );
        match be.capabilities().await {
            Ok(c) => {
                modalities.extend(c.modalities);
                models.extend(c.models);
            }
            Err(e) => tracing::warn!("whisper backend at {url} unavailable: {e}"),
        }
    }
    #[cfg(feature = "comfyui")]
    if let (Some(url), Some(checkpoint)) = (&cfg.comfyui_url, &cfg.comfyui_checkpoint) {
        let be = hive_core::backend::comfyui::ComfyUiBackend::new(url, checkpoint);
        match be.capabilities().await {
            Ok(c) => {
                modalities.extend(c.modalities);
                models.extend(c.models);
            }
            Err(e) => tracing::warn!("comfyui backend at {url} unavailable: {e}"),
        }
    }
    if modalities.is_empty() {
        modalities.push(Modality::Text);
    }
    Ok(Capabilities {
        hardware,
        modalities,
        models,
        // ADR-006 D46/D48: read from node.env (`hive set HIVE_ALLOW_INTERNET|HIVE_TOOLS_LEVEL`,
        // or the desktop Trust pane) — off/sandboxed by default, seeded from the pairing choice.
        allow_internet: cfg.allow_internet,
        tools_level: cfg.tools_level,
        storage_gb_offered: None,
        shard_capable: None,
    })
}

fn hub(cfg: &config::NodeConfig) -> Result<HubClient> {
    Ok(HubClient::new(
        &cfg.hub_url,
        &cfg.anon_key,
        config::require_node_key(cfg)?,
    ))
}

/// Scheduled check-in/out (feature request, Jack, 2026-09-12): does `now` (this machine's own
/// local clock -- a schedule is never stored with a timezone, see migration
/// 20260912270000_node_schedules.sql) fall inside any of this node's configured weekly windows?
/// `schedule` is the raw jsonb from `hub.get_schedule()`: a `[{"day":0-6,"start":"HH:MM","end":"HH:MM"}]`
/// array, or anything else (null, not an array, empty) is treated as "no schedule" by the caller.
fn in_schedule_window(schedule: &serde_json::Value, now: chrono::DateTime<chrono::Local>) -> bool {
    let Some(windows) = schedule.as_array() else {
        return false;
    };
    let today = now.weekday().num_days_from_sunday() as i64; // 0 = Sunday, matches the schema
    let minutes_now = now.hour() as i64 * 60 + now.minute() as i64;
    windows.iter().any(|w| {
        let day = w.get("day").and_then(|v| v.as_i64());
        let start = w.get("start").and_then(|v| v.as_str()).and_then(parse_hhmm);
        let end = w.get("end").and_then(|v| v.as_str()).and_then(parse_hhmm);
        matches!((day, start, end), (Some(d), Some(s), Some(e)) if d == today && minutes_now >= s && minutes_now < e)
    })
}

/// "HH:MM" -> minutes since midnight, or `None` if it doesn't parse (the server already validates
/// this shape on write, but the CLI doesn't trust that blindly -- a malformed window is just
/// skipped rather than panicking a long-running background loop).
fn parse_hhmm(s: &str) -> Option<i64> {
    let (h, m) = s.split_once(':')?;
    Some(h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Kept alive for the whole process: dropping it early would silently truncate the log file.
    let _log_guard = hive_core::logging::init("hive");
    config::export_env(); // node.env → env, so `--model` etc. pick up `hive set HIVE_MODEL …`
    let cli = Cli::parse();
    let cfg = config::load()?;

    match cli.cmd {
        Cmd::Probe => {
            let caps = capabilities(&cfg).await?;
            println!("{}", serde_json::to_string_pretty(&caps)?);
        }
        Cmd::Run {
            prompt,
            backend,
            model,
            max_tokens,
            audio_path,
            language,
            negative_prompt,
        } => {
            let be: Box<dyn Backend> = match backend.as_str() {
                "mock" => Box::new(MockBackend),
                #[cfg(feature = "llama-cpp")]
                "llama_cpp" => Box::new(hive_core::backend::llama_cpp::LlamaCppBackend::new(
                    &cfg.llama_url,
                )),
                #[cfg(feature = "whisper")]
                "whisper" => {
                    let url = cfg.whisper_url.clone().ok_or_else(|| {
                        anyhow::anyhow!(
                            "no whisper backend configured. Run `hive set HIVE_WHISPER_URL http://127.0.0.1:8081`"
                        )
                    })?;
                    let whisper_model = model
                        .clone()
                        .or_else(|| cfg.whisper_model.clone())
                        .unwrap_or_else(|| "unknown".into());
                    Box::new(hive_core::backend::whisper::WhisperCppBackend::new(
                        url,
                        whisper_model,
                    ))
                }
                #[cfg(feature = "comfyui")]
                "comfyui" => {
                    let url = cfg.comfyui_url.clone().ok_or_else(|| {
                        anyhow::anyhow!(
                            "no comfyui backend configured. Run `hive set HIVE_COMFYUI_URL http://127.0.0.1:8188`"
                        )
                    })?;
                    let checkpoint = model
                        .clone()
                        .or_else(|| cfg.comfyui_checkpoint.clone())
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "no checkpoint given. Pass --model or run `hive set HIVE_COMFYUI_CHECKPOINT <file.safetensors>`"
                            )
                        })?;
                    Box::new(hive_core::backend::comfyui::ComfyUiBackend::new(
                        url, checkpoint,
                    ))
                }
                other => anyhow::bail!("unknown backend '{other}'"),
            };
            let input = match backend.as_str() {
                "whisper" => {
                    let path = audio_path.ok_or_else(|| {
                        anyhow::anyhow!("--audio-path is required for --backend whisper")
                    })?;
                    let mut v = serde_json::json!({ "audio_path": path });
                    if let Some(lang) = language {
                        v["language"] = serde_json::Value::String(lang);
                    }
                    v
                }
                "comfyui" => {
                    serde_json::json!({ "prompt": prompt, "negative_prompt": negative_prompt })
                }
                _ => serde_json::json!({ "prompt": prompt, "max_tokens": max_tokens }),
            };
            let job = Job {
                id: uuid::Uuid::new_v4(),
                kind: JobKind::Inference,
                project_id: uuid::Uuid::nil(),
                card_id: None,
                parent: None,
                requirements: Requirements {
                    model_id: model,
                    ..Default::default()
                },
                input,
                resume_from: None,
                created_at: chrono::Utc::now(),
            };
            let started = std::time::Instant::now();
            let mut stream = be.run(&job).await?;
            use futures::StreamExt;
            use std::io::Write;
            let mut usage = None;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                print!("{}", chunk.text);
                std::io::stdout().flush().ok();
                if let Some(u) = chunk.usage {
                    usage = Some(u);
                }
                if chunk.done {
                    break;
                }
            }
            println!();
            let u = usage.unwrap_or_default();
            let tps = if u.compute_seconds > 0.0 {
                u.tokens_out as f64 / u.compute_seconds
            } else {
                0.0
            };
            eprintln!(
                "backend={} tokens_in={} tokens_out={} compute={:.2}s ({:.1} tok/s) wall={:.2}s",
                be.name(),
                u.tokens_in,
                u.tokens_out,
                u.compute_seconds,
                tps,
                started.elapsed().as_secs_f64()
            );
        }
        Cmd::Models => {
            #[cfg(feature = "llama-cpp")]
            {
                let be = hive_core::backend::llama_cpp::LlamaCppBackend::new(&cfg.llama_url);
                for m in be.capabilities().await?.models {
                    println!("{}", m.id);
                }
            }
            #[cfg(not(feature = "llama-cpp"))]
            anyhow::bail!("build with --features llama-cpp");
        }
        Cmd::Status => {
            let me = hub(&cfg)?.whoami().await?;
            println!("{}", serde_json::to_string_pretty(&me)?);
        }
        Cmd::CheckIn { stay, interval } => {
            let h = hub(&cfg)?;
            let caps = capabilities(&cfg).await?;
            let row = h.check_in(&caps, cfg.region.as_deref()).await?;
            println!(
                "checked in as {} ({}) — {} models, {} cores, {:.0} GB RAM, gpu={:?}",
                row.get("display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?"),
                row.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                caps.models.len(),
                caps.hardware.cpu_cores,
                caps.hardware.ram_bytes as f64 / 1e9,
                caps.hardware.gpu_model.as_deref().unwrap_or("none"),
            );
            if stay {
                println!("heartbeating every {interval}s — Ctrl-C to check out");
                // A schedule (if the owning member set one on the web) is enforced from here on;
                // this initial check-in above always happens immediately regardless -- running
                // `hive check-in --stay` by hand is itself a deliberate "I want to work now"
                // action, the schedule only governs the unattended loop that follows.
                let mut checked_in = true;
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(interval));
                loop {
                    tokio::select! {
                        _ = tick.tick() => {
                            let schedule = h.get_schedule().await.ok().flatten();
                            match schedule.as_ref().filter(|s| s.as_array().is_some_and(|a| !a.is_empty())) {
                                Some(sched) => {
                                    let in_window = in_schedule_window(sched, chrono::Local::now());
                                    if in_window && !checked_in {
                                        match h.check_in(&caps, cfg.region.as_deref()).await {
                                            Ok(_) => { checked_in = true; tracing::info!("scheduled check-in"); }
                                            Err(e) => tracing::warn!("scheduled check-in failed: {e}"),
                                        }
                                    } else if !in_window && checked_in {
                                        match h.check_out().await {
                                            Ok(p) => { checked_in = false; tracing::info!("scheduled check-out ({p})"); }
                                            Err(e) => tracing::warn!("scheduled check-out failed: {e}"),
                                        }
                                    } else if checked_in {
                                        if let Err(e) = h.heartbeat().await { tracing::warn!("heartbeat failed: {e}"); }
                                    }
                                    // else: outside the window and already checked out -- idle, nothing to do this tick.
                                }
                                None => {
                                    // No schedule set -- exactly today's behavior, always heartbeat.
                                    match h.heartbeat().await {
                                        Ok(ts) => tracing::info!("heartbeat ok {ts}"),
                                        Err(e) => tracing::warn!("heartbeat failed: {e}"),
                                    }
                                }
                            }
                        }
                        _ = tokio::signal::ctrl_c() => {
                            if checked_in {
                                let p = h.check_out().await?;
                                println!("\nchecked out ({p})");
                            } else {
                                println!("\nalready checked out (outside your schedule)");
                            }
                            break;
                        }
                    }
                }
            }
        }
        Cmd::CheckOut => {
            let p = hub(&cfg)?.check_out().await?;
            println!("checked out ({p})");
        }
        Cmd::Work { poll, model } => {
            #[cfg(not(feature = "llama-cpp"))]
            anyhow::bail!("build with --features llama-cpp");
            #[cfg(feature = "llama-cpp")]
            {
                let h = hub(&cfg)?;
                let caps = capabilities(&cfg).await?;
                if caps.models.is_empty() {
                    anyhow::bail!(
                        "no models available at {} — is Ollama / llama-server running?",
                        cfg.llama_url
                    );
                }
                let row = h.check_in(&caps, cfg.region.as_deref()).await?;
                println!(
                    "working as {} — {} models, polling every {poll}s, Ctrl-C to check out",
                    row.get("display_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?"),
                    caps.models.len()
                );
                let be = hive_core::backend::llama_cpp::LlamaCppBackend::new(&cfg.llama_url);
                let sandbox = hive_core::sandbox::Sandbox::new()
                    .map_err(|e| anyhow::anyhow!("sandbox engine init failed: {e}"))?;
                let w = worker::Worker {
                    hub: &h,
                    backend: &be,
                    caps: &caps,
                    default_model: model,
                    stop: worker::stop_on_signal(),
                    events: None,
                    data_dir: hive_core::sandbox::default_data_dir(),
                    sandbox: Some(&sandbox),
                };
                w.run_forever(
                    std::time::Duration::from_secs(poll),
                    (30 / poll.max(1)).max(1) as u32,
                )
                .await?;
            }
        }
        Cmd::Set { key, value } => {
            let p = config::set(&key, &value)?;
            println!("wrote {key} to {}", p.display());
        }
        Cmd::Pair => {
            use hive_core::hub::{Pairing, PairingPoll};
            if cfg.node_key.is_some() {
                println!(
                    "this machine already has a node key ({}). Remove HIVE_NODE_KEY to re-pair.",
                    config::path().display()
                );
                return Ok(());
            }
            let hw = hive_core::probe::probe_hardware();
            let hint = serde_json::json!({
                "hostname": std::env::var("HOSTNAME").ok().or_else(hostname),
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "cpu": hw.cpu_model,
                "gpu": hw.gpu_model,
                "ram_gb": (hw.ram_bytes as f64 / 1e9).round(),
            });
            let p = Pairing::new(&cfg.hub_url, &cfg.anon_key);
            let start = p.begin(hint).await?;
            println!();
            println!("  Pair this machine with your Hive account.");
            println!();
            println!("  1. Open   {}", start.url);
            println!("  2. Enter  {}", start.code);
            println!();
            println!(
                "  Code expires in {} minutes. Waiting…",
                start.expires_in_seconds / 60
            );
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(3));
            loop {
                tick.tick().await;
                match p.poll(&start.secret).await? {
                    PairingPoll::Pending => continue,
                    PairingPoll::Expired => {
                        anyhow::bail!("code expired before it was claimed — run `hive pair` again")
                    }
                    PairingPoll::Claimed {
                        node_key,
                        node_id,
                        display_name,
                        allow_internet,
                        tools_level,
                    } => {
                        config::set("HIVE_NODE_KEY", &node_key)?;
                        // Seed local config with what was chosen on the pairing page — otherwise
                        // the first check-in would silently reset both to their defaults.
                        config::set(
                            "HIVE_ALLOW_INTERNET",
                            if allow_internet { "true" } else { "false" },
                        )?;
                        config::set(
                            "HIVE_TOOLS_LEVEL",
                            match tools_level {
                                ToolsLevel::InferenceOnly => "inference_only",
                                ToolsLevel::SandboxedTools => "sandboxed_tools",
                            },
                        )?;
                        println!(
                            "\n  Paired as \"{display_name}\" ({node_id}). Key saved to {}.",
                            config::path().display()
                        );
                        println!("  Next: `hive check-in --stay`");
                        break;
                    }
                }
            }
        }
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
