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
    /// ADR-030: submit, poll, or wait on a `code` card from outside Hive's own client apps --
    /// e.g. so Cowork's `device_bash` can point Hive at a directory and say "work here" without
    /// needing Xcode/Swift itself. Needs nothing but `hive pair`; never a Cmd Work credential.
    Card {
        #[command(subcommand)]
        cmd: CardCmd,
    },
    /// ADR-035 C1: Bots chat and agent collaboration. Registers this node as a local agent
    /// (an `AgentProfile` with `runtime_kind: Local`, `preferred_host` pinned to this node's
    /// own id) in the same LocalHub the desktop app's vault uses, or lists agents already
    /// registered. Build with `--features bots` (2026-09-15, Jack: "I want the computers to
    /// show up as agents in Hive that I can engage with directly").
    Bots {
        #[command(subcommand)]
        cmd: BotsCmd,
    },
}

#[derive(Subcommand)]
enum CardCmd {
    /// Create one `code` card in a project you own, the same write the Kanban makes when you
    /// drag a card onto the board -- just from a terminal. `--project` matches by title
    /// (case-insensitive substring; ambiguous or no match is an error naming the candidates).
    /// Exactly one of `--workspace`/`--repo` is required, same as the web form.
    Submit {
        /// Title (or a substring of it) of one of your own local-execution projects.
        #[arg(long)]
        project: String,
        /// What the agent should do.
        #[arg(long)]
        task: String,
        /// Absolute path already on the claiming node (mutually exclusive with `--repo`).
        #[arg(long)]
        workspace: Option<String>,
        /// Git URL to clone instead of using an existing workspace.
        #[arg(long)]
        repo: Option<String>,
        /// Branch/tag/sha, only meaningful with `--repo`.
        #[arg(long)]
        repo_ref: Option<String>,
        /// `local` (default, runs on whichever of your own nodes claims it) or a BYOK cloud
        /// provider (`anthropic`/`openai`/`nous` -- requires a key already set for that
        /// provider and `--cloud-consent`).
        #[arg(long, default_value = "local")]
        brain: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, default_value_t = 40)]
        max_turns: u32,
        /// Required alongside a non-`local` `--brain`; a plain flag, no value.
        #[arg(long)]
        cloud_consent: bool,
        /// Print just the new card's id (e.g. for piping into `hive card await`).
        #[arg(long)]
        quiet: bool,
        /// Makes a retry of the exact same submission idempotent: resubmitting with the same
        /// id returns the original card (or errors if the request itself changed) instead of
        /// creating a duplicate. Omit for a plain one-shot submission (today's default
        /// behavior, unchanged).
        #[arg(long)]
        request_id: Option<uuid::Uuid>,
        /// ADR-032: this card may spawn child cards (spawn_card) and pause itself on one
        /// (wait_for_child). Off by default -- an ordinary `hive card submit` is unaffected.
        #[arg(long)]
        coordinator: bool,
    },
    /// Print one card's current status, title, and latest output (if any) as JSON.
    Status {
        card_id: uuid::Uuid,
    },
    /// Poll a card's status until it leaves `ready`/`running` (or `--timeout` elapses), then
    /// print its final status + output. Useful right after `hive card submit --quiet`.
    Await {
        card_id: uuid::Uuid,
        #[arg(long, default_value_t = 5)]
        poll: u64,
        /// Give up after this many seconds (default: wait indefinitely).
        #[arg(long)]
        timeout: Option<u64>,
    },
}

#[derive(Subcommand)]
enum BotsCmd {
    /// Register this node as a local Bots agent: creates an `AgentProfile` in this node's own
    /// LocalHub with `runtime_kind: Local` and `preferred_host` set to this node's own id
    /// (looked up via `whoami`, so this node must already be paired -- `hive pair` first).
    /// Prints the new agent's id. Running this again creates a second agent, not an update --
    /// there is no dedup on node id yet (fine for today's one-agent-per-machine use; a repeat
    /// run is a caller mistake, not a crash).
    AgentRegister {
        /// Display name for the agent (defaults to this node's own display name from `whoami`).
        #[arg(long)]
        name: Option<String>,
    },
    /// List every Bots agent your account owns, across every node that has registered one.
    AgentList,
    /// Create a group room (ADR-035 C2 Track A) with several of your own agents in it, so
    /// group chat is exercisable from a terminal. The GUI has its own room creation; this exists
    /// because nothing else lets you verify a multi-agent room against a real local model, and
    /// because a terminal path is a useful fallback when a demo machine misbehaves.
    RoomCreate {
        /// Room name, e.g. "Build crew".
        #[arg(long)]
        name: String,
        /// Agent names or ids to add, repeated: `--agent One --agent Two`. Names match
        /// case-insensitively against your own agents; 1-16 of them.
        #[arg(long = "agent", required = true)]
        agents: Vec<String>,
    },
    /// Post into a room, resolving `@Name` in the text against that room's roster exactly the
    /// way the apps do -- so `hive bots say <room> "@One @Two what do you think?"` wakes both.
    /// With no mentions it posts without waking anyone, which is a valid thing to do in a room.
    Say {
        /// Room id, as printed by `room-create`.
        conversation_id: uuid::Uuid,
        /// Message text; `@Name` mentions choose the recipients.
        #[arg(trailing_var_arg = true)]
        text: Vec<String>,
    },
    /// Send a message to one of your own local agents, over its DM conversation -- creating
    /// that conversation on first use. Minimal terminal-only chat loop while there's no UI
    /// (2026-09-15): `hive bots dm --to <agent-id> <text>`, then a `hive bots work` process
    /// (running, possibly elsewhere) drains and replies, then `hive bots read` to see it.
    Dm {
        /// Agent id, as printed by `agent-list`.
        #[arg(long = "to")]
        agent_id: uuid::Uuid,
        /// Message text (everything after the flags, joined with spaces).
        text: Vec<String>,
    },
    /// Read one conversation's messages, oldest first (or newest-since with `--after`).
    Read {
        conversation_id: uuid::Uuid,
        /// Only messages with a server sequence after this one -- for polling a reply.
        #[arg(long)]
        after: Option<u64>,
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Drain this node's pending Bots deliveries until Ctrl-C: for every locally-hosted agent
    /// (`preferred_host` pinned to this node), claim ready deliveries, run a bounded local
    /// turn, and post the reply -- the `DeliveryExecutor` loop (`bots/executor.rs`,
    /// 2026-09-15). Requires `--features bots,llama-cpp` (the latter is in `default`, so a
    /// plain `--features bots` build already has it unless `--no-default-features` was used).
    Work {
        /// Seconds between drain passes.
        #[arg(long, default_value_t = 5)]
        poll: u64,
        /// Local model name the loopback backend should request (same model space as
        /// `hive run`/`hive work`'s `--model`/`HIVE_MODEL`).
        #[arg(long, env = "HIVE_MODEL")]
        model: Option<String>,
    },
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
                // This node's hub round-trip time, as measured on the previous heartbeat -- fed
                // back into the next call so the hub always has a (one-interval-stale) number.
                let mut last_rtt_ms: Option<u64> = None;
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
                                        match h.heartbeat(last_rtt_ms).await {
                                            Ok((_, rtt)) => last_rtt_ms = Some(rtt),
                                            Err(e) => tracing::warn!("heartbeat failed: {e}"),
                                        }
                                    }
                                    // else: outside the window and already checked out -- idle, nothing to do this tick.
                                }
                                None => {
                                    // No schedule set -- exactly today's behavior, always heartbeat.
                                    match h.heartbeat(last_rtt_ms).await {
                                        Ok((ts, rtt)) => {
                                            tracing::info!("heartbeat ok {ts} ({rtt}ms)");
                                            last_rtt_ms = Some(rtt);
                                        }
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
        Cmd::Card { cmd } => {
            let h = hub(&cfg)?;
            match cmd {
                CardCmd::Submit {
                    project,
                    task,
                    workspace,
                    repo,
                    repo_ref,
                    brain,
                    model,
                    max_turns,
                    cloud_consent,
                    quiet,
                    request_id,
                    coordinator,
                } => {
                    if workspace.is_some() == repo.is_some() {
                        anyhow::bail!("pass exactly one of --workspace or --repo");
                    }
                    let projects = h.code_session_projects().await?;
                    let needle = project.to_lowercase();
                    let mut matches: Vec<_> = projects
                        .iter()
                        .filter(|p| p.title.to_lowercase().contains(&needle))
                        .collect();
                    let matched = match matches.len() {
                        0 => anyhow::bail!(
                            "no local-execution project matches '{project}'. Your projects: {}",
                            projects
                                .iter()
                                .map(|p| p.title.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        1 => matches.remove(0),
                        _ => anyhow::bail!(
                            "'{project}' matches more than one project: {} — be more specific",
                            matches
                                .iter()
                                .map(|p| p.title.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    };
                    let result = h
                        .code_session_submit(
                            matched.id,
                            &task,
                            workspace.as_deref(),
                            repo.as_deref(),
                            repo_ref.as_deref(),
                            &brain,
                            model.as_deref(),
                            max_turns,
                            cloud_consent,
                            request_id,
                            coordinator,
                        )
                        .await?;
                    if quiet {
                        println!("{}", result.card_id);
                    } else {
                        println!(
                            "submitted to \"{}\" — card {}",
                            matched.title, result.card_id
                        );
                        println!("next: `hive card await {}`", result.card_id);
                    }
                }
                CardCmd::Status { card_id } => {
                    let status = h.code_session_status(card_id).await?;
                    println!("{}", serde_json::to_string_pretty(&status)?);
                }
                CardCmd::Await {
                    card_id,
                    poll,
                    timeout,
                } => {
                    if poll == 0 {
                        anyhow::bail!(
                            "--poll must be at least 1 (0 would busy-loop and panics \
                             tokio::time::interval outright)"
                        );
                    }
                    let started = std::time::Instant::now();
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(poll));
                    loop {
                        tick.tick().await;
                        let status = h.code_session_status(card_id).await?;
                        eprintln!("{} — {}", status.status, status.title);
                        // `waiting_on_child` is a normal, self-resolving mid-flight state (the
                        // card is blocked on a child card it spawned, not on anything the caller
                        // needs to act on) -- Sif's review caught this loop treating it as
                        // finished and printing/exiting a card that was still actually running.
                        if !matches!(status.status.as_str(), "ready" | "running" | "waiting_on_child") {
                            println!("{}", serde_json::to_string_pretty(&status)?);
                            break;
                        }
                        if let Some(t) = timeout {
                            if started.elapsed().as_secs() >= t {
                                anyhow::bail!(
                                    "timed out after {t}s waiting on card {card_id} (still {})",
                                    status.status
                                );
                            }
                        }
                    }
                }
            }
        }
        Cmd::Bots { cmd } => {
            #[cfg(feature = "bots")]
            {
                use hive_core::bots::{
                    AgentRuntimeKind, BotsService, ConversationKind, DeliveryExecutor,
                    LocalBotsTurnRunner, LocalModelTurnRunner, MessageKind, MessagePage,
                    NewAgentProfile, NewConversation, NewMessage, Principal, StorageScope,
                };
                use hive_core::local_hub::LocalHubStore;
                let store = std::sync::Arc::new(
                    LocalHubStore::open(config::path().with_file_name("vault-host.sqlite3"))
                        .map_err(|e| anyhow::anyhow!("opening local Bots store: {e}"))?,
                );
                match cmd {
                    BotsCmd::AgentRegister { name } => {
                        let me = hub(&cfg)?.whoami().await?;
                        let draft = NewAgentProfile {
                            owner: me.member_id,
                            name: name.unwrap_or_else(|| me.display_name.clone()),
                            runtime_kind: AgentRuntimeKind::Local,
                            preferred_host: Some(me.node_id),
                            // No policy/UI to pick a real one yet (C1 has no FFI/UI -- see
                            // `bots/mod.rs`'s own doc) -- "default" is a placeholder capability
                            // policy reference, not a real vault lookup.
                            capability_policy_ref: "default".to_string(),
                            provider_account_ref: None,
                            memory_namespace: format!("agent:{}", me.node_id),
                        };
                        let agent = store
                            .agents_create(draft)
                            .await
                            .map_err(|e| anyhow::anyhow!("creating agent profile: {e}"))?;
                        println!(
                            "registered \"{}\" as agent {} (runtime=local, preferred_host={})",
                            agent.name, agent.id, me.node_id
                        );
                    }
                    BotsCmd::AgentList => {
                        let me = hub(&cfg)?.whoami().await?;
                        let agents = store
                            .agents_list(me.member_id)
                            .await
                            .map_err(|e| anyhow::anyhow!("listing agent profiles: {e}"))?;
                        if agents.is_empty() {
                            println!(
                                "no Bots agents registered yet — try `hive bots agent-register`"
                            );
                        }
                        for a in agents {
                            println!(
                                "{}  {:<20}  runtime={:?}  preferred_host={}",
                                a.id,
                                a.name,
                                a.runtime_kind,
                                a.preferred_host
                                    .map(|h| h.to_string())
                                    .unwrap_or_else(|| "-".to_string()),
                            );
                        }
                    }
                    BotsCmd::RoomCreate { name, agents } => {
                        let me = hub(&cfg)?.whoami().await?;
                        if agents.is_empty() || agents.len() > 16 {
                            anyhow::bail!("a room takes between 1 and 16 agents");
                        }
                        let owned = store
                            .agents_list(me.member_id)
                            .await
                            .map_err(|e| anyhow::anyhow!("listing agents: {e}"))?;
                        let mut chosen = Vec::new();
                        for wanted in &agents {
                            let matches: Vec<_> = owned
                                .iter()
                                .filter(|a| {
                                    !a.archived
                                        && (a.name.eq_ignore_ascii_case(wanted)
                                            || a.id.to_string() == *wanted)
                                })
                                .collect();
                            match matches.as_slice() {
                                [one] => chosen.push((*one).clone()),
                                [] => anyhow::bail!(
                                    "no agent of yours matches {wanted:?} -- see `hive bots agent-list`"
                                ),
                                many => anyhow::bail!(
                                    "{wanted:?} matches {} of your agents; use an id instead",
                                    many.len()
                                ),
                            }
                        }
                        let room = store
                            .conversations_create(NewConversation {
                                title: Some(name.clone()),
                                owner: me.member_id,
                                kind: ConversationKind::Team,
                                project_id: None,
                                coordinator: None,
                                storage_scope: StorageScope::LocalOnly,
                            })
                            .await
                            .map_err(|e| anyhow::anyhow!("creating room: {e}"))?;
                        for agent in &chosen {
                            store
                                .conversations_join(Principal::Agent(agent.id), room.id)
                                .await
                                .map_err(|e| {
                                    anyhow::anyhow!("adding {} to the room: {e}", agent.name)
                                })?;
                        }
                        println!("room {} \"{}\" with {}", room.id, name,
                            chosen.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "));
                        // Stated rather than discovered mid-demo: a BYOK agent has no runner yet.
                        let unroutable: Vec<&str> = chosen
                            .iter()
                            .filter(|a| a.runtime_kind != AgentRuntimeKind::Local)
                            .map(|a| a.name.as_str())
                            .collect();
                        if !unroutable.is_empty() {
                            println!(
                                "note: {} cannot reply yet -- only Local agents have a turn runner, so a delivery to them stays pending",
                                unroutable.join(", ")
                            );
                        }
                        println!("next: `hive bots say {} \"@{} hello\"`, with `hive bots work` running", room.id,
                            chosen.first().map(|a| a.name.clone()).unwrap_or_default());
                    }
                    BotsCmd::Say { conversation_id, text } => {
                        let me = hub(&cfg)?.whoami().await?;
                        let text = text.join(" ");
                        if text.trim().is_empty() {
                            anyhow::bail!("message text is required");
                        }
                        let room = store
                            .conversations_list(Principal::User(me.member_id))
                            .await
                            .map_err(|e| anyhow::anyhow!("listing conversations: {e}"))?
                            .into_iter()
                            .find(|c| c.id == conversation_id)
                            .ok_or_else(|| anyhow::anyhow!("no room of yours with that id"))?;
                        let roster = store
                            .bots_room_agents(Principal::User(me.member_id), room.id)
                            .map_err(|e| anyhow::anyhow!("reading the room roster: {e}"))?;
                        let mentions = hive_core::bots::resolve_mentions(
                            &text,
                            &roster,
                            Principal::User(me.member_id),
                        );
                        if !mentions.unresolved.is_empty() {
                            println!("unrecognized name(s), nobody notified for them: {}",
                                mentions.unresolved.join(", "));
                        }
                        let named: Vec<String> = mentions
                            .recipients
                            .iter()
                            .filter_map(|id| roster.iter().find(|a| a.id == *id))
                            .map(|a| a.name.clone())
                            .collect();
                        let sent = store
                            .message_send(
                                Principal::User(me.member_id),
                                room.id,
                                uuid::Uuid::new_v4().to_string(),
                                room.policy_revision,
                                mentions.recipients.clone(),
                                NewMessage {
                                    thread_root: None,
                                    kind: MessageKind::Text,
                                    body: Some(text),
                                    attachment_refs: Vec::new(),
                                    task_ref: None,
                                    turn_ref: None,
                                    source_event_ref: None,
                                },
                            )
                            .await
                            .map_err(|e| anyhow::anyhow!("sending message: {e}"))?;
                        if named.is_empty() {
                            println!("posted (seq {}) -- no mentions, so nobody was woken", sent.server_sequence);
                        } else {
                            println!("posted (seq {}) -- woke {}; `hive bots read {}` for replies",
                                sent.server_sequence, named.join(", "), room.id);
                        }
                    }
                    BotsCmd::Dm { agent_id, text } => {
                        let me = hub(&cfg)?.whoami().await?;
                        let text = text.join(" ");
                        if text.trim().is_empty() {
                            anyhow::bail!("message text is required");
                        }
                        let conversations = store
                            .conversations_list(Principal::User(me.member_id))
                            .await
                            .map_err(|e| anyhow::anyhow!("listing conversations: {e}"))?;
                        let conversation = match conversations.into_iter().find(|c| {
                            c.kind == ConversationKind::AgentDm
                                && c.coordinator == Some(agent_id)
                        }) {
                            Some(c) => c,
                            None => store
                                .conversations_create(NewConversation {
                    title: None,
                                    owner: me.member_id,
                                    kind: ConversationKind::AgentDm,
                                    project_id: None,
                                    coordinator: Some(agent_id),
                                    storage_scope: StorageScope::LocalOnly,
                                })
                                .await
                                .map_err(|e| {
                                    anyhow::anyhow!("creating DM conversation: {e}")
                                })?,
                        };
                        let sent = store
                            .message_send(
                                Principal::User(me.member_id),
                                conversation.id,
                                uuid::Uuid::new_v4().to_string(),
                                conversation.policy_revision,
                                vec![agent_id],
                                NewMessage {
                                    thread_root: None,
                                    kind: MessageKind::Text,
                                    body: Some(text),
                                    attachment_refs: Vec::new(),
                                    task_ref: None,
                                    turn_ref: None,
                                    source_event_ref: None,
                                },
                            )
                            .await
                            .map_err(|e| anyhow::anyhow!("sending message: {e}"))?;
                        println!(
                            "sent to conversation {} (seq {}) -- run `hive bots work` if it isn't already, then `hive bots read {}` to see the reply",
                            conversation.id, sent.server_sequence, conversation.id
                        );
                    }
                    BotsCmd::Read {
                        conversation_id,
                        after,
                        limit,
                    } => {
                        let me = hub(&cfg)?.whoami().await?;
                        let messages = store
                            .messages_list(
                                Principal::User(me.member_id),
                                conversation_id,
                                MessagePage {
                                    before: None,
                                    after,
                                    limit,
                                },
                            )
                            .await
                            .map_err(|e| anyhow::anyhow!("listing messages: {e}"))?;
                        if messages.is_empty() {
                            println!("no messages yet");
                        }
                        for m in messages {
                            let who = if m.kind == MessageKind::System { "System".to_string() } else { match m.author {
                                Principal::User(_) => "you".to_string(),
                                Principal::Agent(id) => format!("agent {id}"),
                            }};
                            println!(
                                "[{}] {}: {}",
                                m.server_sequence,
                                who,
                                m.body.as_deref().unwrap_or("(no body)")
                            );
                        }
                    }
                    BotsCmd::Work { poll, model } => {
                        #[cfg(not(feature = "llama-cpp"))]
                        {
                            let _ = (poll, model);
                            anyhow::bail!("build with --features bots,llama-cpp");
                        }
                        #[cfg(feature = "llama-cpp")]
                        {
                            let me = hub(&cfg)?.whoami().await?;
                            // No pre-drain report: `drain_once` reports after each pass now, so a
                            // cloud agent that just replied cannot also collect an "unsupported"
                            // notice. Reporting here would fire before the first drain ever ran.
                            let model = model.ok_or_else(|| {
                                anyhow::anyhow!(
                                    "--model (or HIVE_MODEL) is required for `hive bots work`"
                                )
                            })?;
                            let runner: std::sync::Arc<dyn LocalBotsTurnRunner> =
                                std::sync::Arc::new(
                                    LocalModelTurnRunner::loopback(
                                        me.node_id,
                                        model,
                                        &cfg.llama_url,
                                    )
                                    .map_err(|e| {
                                        anyhow::anyhow!("constructing local turn runner: {e}")
                                    })?,
                                );
                            let mut executor = DeliveryExecutor::new(
                                store.clone(),
                                runner,
                                me.node_id,
                                me.member_id,
                            );
                            // BYOK provider agents (Claude, Nous) answer through the hub, because
                            // the key is resolved hub-side and never reaches this machine
                            // (ADR-008). Owner and node key come from verified configuration
                            // here -- `whoami` for the member, `cfg.node_key` for the credential
                            // -- never from anything a conversation supplied.
                            let mut cloud = "not configured (no node key)".to_string();
                            if let Some(raw_key) = cfg.node_key.clone() {
                                match hive_core::bots::CloudTurnRunner::new(
                                    &cfg.hub_url,
                                    cfg.anon_key.clone(),
                                    raw_key,
                                    me.member_id,
                                ) {
                                    Ok(runner) => {
                                        executor = executor
                                            .with_cloud_runner(std::sync::Arc::new(runner));
                                        cloud = "enabled".to_string();
                                    }
                                    // Not fatal: local agents must keep working on a node that
                                    // cannot reach the hub. The unroutable notice will say so in
                                    // the room rather than leaving Claude silent.
                                    Err(e) => cloud = format!("unavailable ({e})"),
                                }
                            }
                            println!(
                                "draining Bots deliveries for agents hosted on this node, polling every {poll}s, Ctrl-C to stop\ncloud provider agents (Claude/Nous): {cloud}"
                            );
                            let mut stop = worker::stop_on_signal();
                            loop {
                                if *stop.borrow() {
                                    break;
                                }
                                let summary = executor.drain_once().await;
                                if summary.delivered + summary.failed + summary.requeued > 0 {
                                    println!(
                                        "checked {} agent(s): delivered={} failed={} requeued={}",
                                        summary.agents_checked,
                                        summary.delivered,
                                        summary.failed,
                                        summary.requeued,
                                    );
                                }
                                tokio::select! {
                                    _ = tokio::time::sleep(std::time::Duration::from_secs(poll)) => {}
                                    _ = stop.changed() => {}
                                }
                            }
                            println!("checked out");
                        }
                    }
                }
            }
            #[cfg(not(feature = "bots"))]
            {
                let _ = cmd;
                anyhow::bail!("build with --features bots");
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
