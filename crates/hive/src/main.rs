//! `hive` — OH Hive node CLI (ADR-003 D66). Subcommands map to what the
//! desktop app does in its GUI (ADR-010), so a headless Linux box can be a
//! compute node without Tauri.

mod config;
mod worker;

use anyhow::Result;
use clap::{Parser, Subcommand};
use ohhive_core::backend::{mock::MockBackend, Backend};
use ohhive_core::capability::{Capabilities, Modality, Requirements, ToolsLevel};
use ohhive_core::hub::HubClient;
use ohhive_core::job::{Job, JobKind};

#[derive(Parser)]
#[command(name = "hive", version = ohhive_core::VERSION, about = "OH Hive node")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Probe hardware + backends and print the capabilities this node would advertise.
    Probe,
    /// Run one prompt through a backend locally (no coordinator).
    Run {
        prompt: String,
        /// `mock` or `llama_cpp`.
        #[arg(long, default_value = "llama_cpp")]
        backend: String,
        #[arg(long, env = "HIVE_MODEL")]
        model: Option<String>,
        #[arg(long, default_value_t = 256)]
        max_tokens: u64,
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
    let hardware = ohhive_core::probe::probe_hardware();
    let mut modalities = vec![];
    let mut models = vec![];
    #[cfg(feature = "llama-cpp")]
    {
        let be = ohhive_core::backend::llama_cpp::LlamaCppBackend::new(&cfg.llama_url);
        match be.capabilities().await {
            Ok(c) => {
                modalities.extend(c.modalities);
                models.extend(c.models);
            }
            Err(e) => tracing::warn!("llama_cpp backend at {} unavailable: {e}", cfg.llama_url),
        }
    }
    if modalities.is_empty() {
        modalities.push(Modality::Text);
    }
    Ok(Capabilities {
        hardware,
        modalities,
        models,
        allow_internet: false, // ADR-006 D46: off until the member opts in (desktop Trust pane / `hive set`)
        tools_level: ToolsLevel::SandboxedTools,
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
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
        } => {
            let be: Box<dyn Backend> = match backend.as_str() {
                "mock" => Box::new(MockBackend),
                #[cfg(feature = "llama-cpp")]
                "llama_cpp" => Box::new(ohhive_core::backend::llama_cpp::LlamaCppBackend::new(
                    &cfg.llama_url,
                )),
                other => anyhow::bail!("unknown backend '{other}'"),
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
                input: serde_json::json!({ "prompt": prompt, "max_tokens": max_tokens }),
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
                let be = ohhive_core::backend::llama_cpp::LlamaCppBackend::new(&cfg.llama_url);
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
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(interval));
                loop {
                    tokio::select! {
                        _ = tick.tick() => {
                            match h.heartbeat().await {
                                Ok(ts) => tracing::info!("heartbeat ok {ts}"),
                                Err(e) => tracing::warn!("heartbeat failed: {e}"),
                            }
                        }
                        _ = tokio::signal::ctrl_c() => {
                            let p = h.check_out().await?;
                            println!("\nchecked out ({p})");
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
                let be = ohhive_core::backend::llama_cpp::LlamaCppBackend::new(&cfg.llama_url);
                let w = worker::Worker {
                    hub: &h,
                    backend: &be,
                    caps: &caps,
                    default_model: model,
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
            use ohhive_core::hub::{Pairing, PairingPoll};
            if cfg.node_key.is_some() {
                println!(
                    "this machine already has a node key ({}). Remove HIVE_NODE_KEY to re-pair.",
                    config::path().display()
                );
                return Ok(());
            }
            let hw = ohhive_core::probe::probe_hardware();
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
                    } => {
                        config::set("HIVE_NODE_KEY", &node_key)?;
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
