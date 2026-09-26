//! `hive` — OH Hive node CLI (ADR-003 D66). Subcommands map to what the
//! desktop app does in its GUI (ADR-010), so a headless Linux box can be a
//! compute node without Tauri.

mod acceptance;
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
        /// Work against a hub on ANOTHER machine instead of this one's own vault (e.g.
        /// `http://192.168.1.50:8787`). Pair once with `hive hub pair` first; the credentials
        /// saved by that command are what authenticate here.
        ///
        /// This is what lets a machine's agent answer into the Den running somewhere else: the
        /// agents live in the hub machine's vault, and this machine drains only the deliveries
        /// for agents whose `preferred_host` is itself -- enforced by the hub, not by this flag.
        /// Reads `HIVE_BOTS_HUB` when the flag is absent, which is what lets a service
        /// definition carry it. A systemd unit cannot express "pass --hub only if set" without
        /// wrapping the whole ExecStart in a shell, and a plist cannot express it at all.
        #[arg(long, global = false, env = "HIVE_BOTS_HUB")]
        hub: Option<String>,
    },
    /// ADR-025 local hub: serve this machine's vault to your other machines, or pair this
    /// machine with one that is serving. Nothing here touches Supabase.
    Hub {
        #[command(subcommand)]
        cmd: HubCmd,
    },
}

#[derive(Subcommand)]
enum HubCmd {
    /// Serve this machine's vault so your other paired machines can reach it. Binds loopback by
    /// default; a LAN address is allowed, a wildcard or public one is refused by the transport
    /// itself. Runs until Ctrl-C.
    ///
    /// ALWAYS PREFER A WIRED (ETHERNET) ADDRESS OVER WI-FI. If this machine has both, bind the
    /// wired one. A machine with Ethernet and Wi-Fi on the same subnet has two addresses but
    /// only one default route, and binding the address that is *not* on the default route means
    /// requests arrive on one interface and replies leave by another. Switches and firewalls
    /// drop that asymmetry unpredictably, so peers see connections that work, then don't, then
    /// do -- with no error anywhere that names the cause. Wired is also what you want for a hub
    /// other machines depend on: no roaming, no power-saving, no shared airtime.
    Serve {
        /// Address to bind. Use this machine's WIRED LAN address (e.g. `192.168.1.50:8787`) so
        /// other machines can reach it -- prefer Ethernet over Wi-Fi whenever both exist, and
        /// bind the address that carries the default route. The default here is loopback only,
        /// which is useful for a local smoke test and reachable by nothing else.
        /// Reads `HIVE_HUB_BIND` when the flag is absent, so a service definition can carry
        /// the address without a wrapper shell.
        #[arg(long, default_value = "127.0.0.1:8787", env = "HIVE_HUB_BIND")]
        bind: String,
    },
    /// Give this hub a friendly name that members show instead of its IP address.
    /// With no name, prints the current one.
    Name {
        /// e.g. "Asgard"
        name: Option<String>,
    },
    /// Print a single-use pairing code for another machine to redeem. Run this on the machine
    /// that is serving.
    PairCode,
    /// Confirm, on the HUB machine, that a paired node belongs to your Hive account -- the
    /// consent step that pairing deliberately does not perform on its own.
    ///
    /// Pairing proves someone had a single-use code. It does not decide whose agents that
    /// machine may act for, and `vault.rs` grants nothing on pairing alone by design. This is
    /// where you say "yes, that is my machine": it writes your member id, taken from this
    /// machine's own verified Hive account, onto that node. Until then the node authenticates
    /// fine and can read nothing.
    Adopt {
        /// The node id `hive hub pair` printed on the other machine.
        #[arg(long)]
        node: uuid::Uuid,
    },
    /// Pair THIS machine with a hub another machine is serving, and save the credentials it
    /// issues. Run `hive hub pair-code` on the hub machine to get the code.
    Pair {
        /// The hub's origin, e.g. `http://192.168.1.50:8787`.
        #[arg(long)]
        hub: String,
        /// The single-use code from `hive hub pair-code` on the hub machine.
        #[arg(long)]
        code: String,
        /// How this machine should appear to the hub. Defaults to its hostname.
        #[arg(long)]
        name: Option<String>,
    },
    /// Native periodic wake-and-reply for one of your Bots agents, replacing what an external
    /// scheduler (Paperclip's heartbeat) previously stood in for -- see
    /// `local_hub/schedules.rs` for why owning this natively matters (2026-09-24: a heartbeat
    /// reply re-triggering its own wake built a real incident). Runs only while `hive hub serve`
    /// is running on this machine; `--agent` is an id from `hive bots agent-list`.
    Schedule {
        #[command(subcommand)]
        cmd: ScheduleCmd,
    },
    /// Grant a locally-hosted agent write access for the `web_post_json` tool, and configure the
    /// secret it substitutes for the literal placeholder "{{SECRET}}" in a request body. Same
    /// local-vault ownership resolution as `hub schedule` (see `bots_agent_web_post_hosts_grant_local`) --
    /// no Hive-account node-key pairing needed on the hub-serving machine. The agent's own model
    /// never sees the secret value: it writes the placeholder, and this is substituted server-side.
    WebTool {
        #[command(subcommand)]
        cmd: WebToolCmd,
    },
    /// Set, clear, or view a locally-hosted agent's monthly token budget. Record + report only
    /// in v1: an agent over budget still runs, this just tells you it happened. Same local-vault
    /// ownership resolution as `hub schedule`/`hub web-tool` -- no Hive-account node-key pairing
    /// needed on the hub-serving machine. Usage is recorded per completed turn by whichever
    /// runner ran it (local, library-tool, or cloud/BYOK); a fleet-hosted agent's turns are not
    /// tracked here in v1.
    Budget {
        #[command(subcommand)]
        cmd: BudgetCmd,
    },
}

#[derive(Subcommand)]
enum WebToolCmd {
    /// Add one or more hosts to an agent's web-post allowlist (union with whatever is already
    /// granted). A read-only `web_fetch` grant is separate and unaffected.
    GrantPostHost {
        /// Agent id, as printed by `hive bots agent-list`.
        #[arg(long)]
        agent: uuid::Uuid,
        /// Bare host names, e.g. `script.google.com`.
        #[arg(required = true)]
        hosts: Vec<String>,
    },
    /// Store (or replace) the secret substituted for "{{SECRET}}" in a `web_post_json` body sent
    /// to `--host` on `--agent`'s behalf. Not readable back from the CLI or any agent-facing RPC.
    SetSecret {
        /// Agent id, as printed by `hive bots agent-list`.
        #[arg(long)]
        agent: uuid::Uuid,
        /// Bare host name this secret is scoped to.
        #[arg(long)]
        host: String,
        /// The secret value. Passed on the command line like any other flag here -- see the
        /// standing note about always running from a fresh terminal.
        #[arg(long)]
        token: String,
    },
}

#[derive(Subcommand)]
enum BudgetCmd {
    /// Set (or replace) an agent's monthly token cap.
    Set {
        /// Agent id, as printed by `hive bots agent-list`.
        #[arg(long)]
        agent: uuid::Uuid,
        /// Cap for the current and future UTC calendar months, in tokens (prompt + completion).
        #[arg(long)]
        monthly_tokens: i64,
    },
    /// Remove an agent's monthly token cap (usage keeps being recorded either way).
    Clear {
        /// Agent id, as printed by `hive bots agent-list`.
        #[arg(long)]
        agent: uuid::Uuid,
    },
    /// Show one agent's usage and cap for the current UTC month.
    Status {
        /// Agent id, as printed by `hive bots agent-list`.
        #[arg(long)]
        agent: uuid::Uuid,
    },
    /// List every agent that has a cap set or has recorded usage this UTC month.
    List,
}

#[derive(Subcommand)]
enum ScheduleCmd {
    /// Create a schedule: every `--every` seconds, send `text` to `--agent`'s DM conversation
    /// (created on first use) and record its reply. One attempt per occurrence, no retries; a
    /// missed run (hub was down) is skipped forward to the latest interval, not backfilled.
    Create {
        /// Short label, shown by `list`.
        #[arg(long)]
        name: String,
        /// Agent id, as printed by `hive bots agent-list`.
        #[arg(long)]
        agent: uuid::Uuid,
        /// Interval in seconds, 60..=2592000 (30 days). E.g. 14400 for "every 4 hours".
        #[arg(long)]
        every: i64,
        /// When the first occurrence should fire, as an RFC3339 timestamp (e.g.
        /// 2026-09-28T13:00:00Z or 2026-09-28T06:00:00-07:00). Every later occurrence stays
        /// locked to this same instant plus a whole number of intervals, so this is how you get
        /// e.g. "every 7 days, starting this Sunday 6am" instead of "7 days from whenever I
        /// happened to run this command". Omit to keep the old behavior: first occurrence one
        /// interval from now. A timestamp in the past is accepted -- the next tick just fires it
        /// right away, same as any other overdue occurrence.
        #[arg(long)]
        start_at: Option<String>,
        /// The message to send each time it fires.
        #[arg(trailing_var_arg = true)]
        text: Vec<String>,
    },
    /// List your schedules.
    List,
    /// Stop a schedule from firing (its occurrence history is kept).
    Disable {
        /// Schedule id, as printed by `list`.
        id: uuid::Uuid,
    },
    /// Resume a schedule stopped with `disable`.
    Enable {
        /// Schedule id, as printed by `list`.
        id: uuid::Uuid,
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
        #[command(flatten)]
        checks: Box<acceptance::CheckArgs>,
        /// Only this private-fleet node may claim the job. Waits if offline; no fallback.
        #[arg(long = "node", value_name = "NODE_UUID")]
        target_node_id: Option<uuid::Uuid>,
    },
    /// Print one card's current status, title, and latest output (if any) as JSON.
    Status { card_id: uuid::Uuid },
    /// Poll a card's status until it leaves `ready`/`running` (or `--timeout` elapses), then
    /// print its final status + output. Useful right after `hive card submit --quiet`.
    Await {
        card_id: uuid::Uuid,
        #[arg(long, default_value_t = 5)]
        poll: u64,
        /// Give up after this many seconds (default: wait indefinitely).
        #[arg(long)]
        timeout: Option<u64>,
        /// Exit non-zero unless the host's acceptance receipt says exactly this. There is
        /// deliberately no `none` value: a card with no receipt fails every expectation, because
        /// an absent verdict is not a passing one. Use this in a script that needs the gate's
        /// answer rather than the model's own account of itself.
        #[arg(long, value_parser = ["passed", "failed", "errored", "unverified", "skipped"])]
        expect_acceptance: Option<String>,
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
    /// Archive an agent so it stops appearing in the roster and stops receiving deliveries.
    ///
    /// `agent-register` has no dedup, so a machine registered more than once leaves duplicates
    /// behind, and before this there was no way to remove one from a terminal at all -- the
    /// roster only ever grew. Archiving is reversible in the store and does not delete history.
    AgentArchive {
        /// The agent id from `agent-list`.
        #[arg(long)]
        agent: uuid::Uuid,
    },
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
    // Advertise only what this build can actually execute.
    //
    // Backends report what they can *produce*, and we used to union that straight into the
    // advertisement -- so configuring ComfyUI made this node advertise `image`, the hub's
    // `node_claim_card` matched on the advertisement alone, and the node claimed an image card
    // it had no executor for. `Worker::run_card` now refuses an unexecutable modality instead
    // of falling through to the text loop, which stops the silent-wrong-output half of that
    // bug; this stops the node asking for the work in the first place, so the card stays
    // claimable by a node that can genuinely do it rather than bouncing off this one.
    //
    // Keep this list in step with `run_card`'s match arms. `speech` is feature-gated in exactly
    // the same way there, and the two must agree: advertising a modality `run_card` will refuse
    // is the bug this exists to prevent, in the other direction.
    let executable = |m: &Modality| match m {
        Modality::Text | Modality::Code => true,
        Modality::Speech => cfg!(feature = "whisper"),
        // No executor exists for these on any build today -- `Worker.backend` is a single
        // llama.cpp handle, not a per-modality registry. When that changes, change this.
        Modality::Image | Modality::Video | Modality::Music => false,
    };
    let dropped: Vec<_> = modalities
        .iter()
        .filter(|m| !executable(m))
        .cloned()
        .collect();
    if !dropped.is_empty() {
        tracing::warn!(
            "not advertising {dropped:?}: a backend reports it can produce them, but this \
             worker has no executor for them and would refuse any such card it claimed"
        );
        modalities.retain(executable);
    }
    if modalities.is_empty() {
        modalities.push(Modality::Text);
    }
    hive_core::model_fit::filter_models(&hardware, &mut models);
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
        acceptance: hive_core::capability::Capabilities::RUNS_ACCEPTANCE,
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

/// Report stored usage beside the receipt; missing counts do not establish zero cost.
fn report_cost(status: &hive_core::hub::CardStatus) {
    let model = status.model_id.as_deref().unwrap_or("unknown model");
    let Some(usage) = &status.usage else {
        eprintln!("usage: unavailable for this card ({model}); cost cannot be determined from this record.");
        return;
    };
    eprintln!(
        "usage: {} in / {} out tokens, {:.1}s compute ({model})",
        usage.tokens_in, usage.tokens_out, usage.compute_seconds
    );
    if usage.tokens_in == 0 && usage.tokens_out == 0 && usage.compute_seconds == 0.0 {
        eprintln!("No usage recorded; these zeros do not establish that the card was free.");
    }
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
                        // `hive work`, not `hive check-in --stay`. Both heartbeat, so both
                        // make the node look present on the site -- but `check-in` only
                        // publishes capabilities and never claims a card, so following it
                        // leaves a member visibly online and permanently idle, with nothing
                        // anywhere saying why. This line is the last instruction a new member
                        // gets and the most likely one to be followed literally.
                        println!("  Next: `hive work`   (checks in, then claims and runs cards)");
                        println!(
                            "  Keep it running after you log out: `scripts/service-mac.sh worker`\n                               on macOS, or the systemd unit in docs/JOIN.md on Linux."
                        );
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
                    checks,
                    target_node_id,
                } => {
                    let checks = (*checks).into_checks()?;
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
                            &checks,
                            target_node_id,
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
                    // The receipt is buried in the report text rather than being a field of its
                    // own, so nothing surfaces it unless someone goes looking. Printed to stderr
                    // so stdout stays exactly the one JSON document callers pipe into.
                    acceptance::report_acceptance(status.latest_output.as_deref());
                    report_cost(&status);
                }
                CardCmd::Await {
                    card_id,
                    poll,
                    timeout,
                    expect_acceptance,
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
                        if !matches!(
                            status.status.as_str(),
                            "ready" | "running" | "waiting_on_child"
                        ) {
                            println!("{}", serde_json::to_string_pretty(&status)?);
                            let outcome =
                                acceptance::report_acceptance(status.latest_output.as_deref());
                            report_cost(&status);
                            if let Some(expected) = &expect_acceptance {
                                let got = outcome.as_ref().map(acceptance::status_word);
                                if got != Some(expected.as_str()) {
                                    anyhow::bail!(
                                        "--expect-acceptance {expected}, but the host's receipt \
                                         for card {card_id} says {}. The card's own status is \
                                         {} -- a card can reach `review` with its checks \
                                         unverified, which is exactly what this flag is for.",
                                        got.unwrap_or("there is no receipt"),
                                        status.status
                                    );
                                }
                            }
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
        Cmd::Hub { cmd } => {
            #[cfg(feature = "bots")]
            {
                use hive_core::local_hub::{serve, LocalHubStore, RemoteLocalHub};
                let db = config::path().with_file_name("vault-host.sqlite3");
                match cmd {
                    HubCmd::Serve { bind } => {
                        let store = LocalHubStore::open(&db)
                            .map_err(|e| anyhow::anyhow!("opening local hub store: {e}"))?;
                        let listener = tokio::net::TcpListener::bind(&bind)
                            .await
                            .map_err(|e| anyhow::anyhow!("binding {bind}: {e}"))?;
                        println!(
                            "local hub serving {} on {} -- Ctrl-C to stop",
                            db.display(),
                            listener.local_addr()?
                        );
                        println!(
                            "on another machine: hive hub pair --hub http://<this-machine>:{} --code <code>",
                            listener.local_addr()?.port()
                        );
                        // Said out loud at the moment it matters, because the failure it
                        // prevents does not look like a network problem from any single
                        // vantage point: peers get intermittent connect failures while a
                        // one-shot from the same machine succeeds, and nothing on either side
                        // reports a cause. Prefer the wired address; bind the one that carries
                        // this machine's default route.
                        if !listener.local_addr()?.ip().is_loopback() {
                            println!(
                                "note: prefer this machine's WIRED (Ethernet) address over Wi-Fi. \
                                 If both are on the same subnet, bind the one carrying the \
                                 default route -- binding the other makes replies leave by a \
                                 different interface than requests arrived on, which peers see \
                                 as connections that work intermittently for no visible reason."
                            );
                        }
                        serve(store, listener, async {
                            let _ = tokio::signal::ctrl_c().await;
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("local hub stopped: {e}"))?;
                    }
                    HubCmd::Name { name } => {
                        let store = LocalHubStore::open(&db)
                            .map_err(|e| anyhow::anyhow!("opening local hub store: {e}"))?;
                        match name {
                            Some(n) => println!(
                                "{}",
                                store
                                    .set_hub_name(&n)
                                    .map_err(|e| anyhow::anyhow!("naming hub: {e}"))?
                            ),
                            None => println!(
                                "{}",
                                store
                                    .hub_name()
                                    .map_err(|e| anyhow::anyhow!("reading hub name: {e}"))?
                                    .unwrap_or_else(|| "(unnamed)".into())
                            ),
                        }
                    }
                    HubCmd::PairCode => {
                        let store = LocalHubStore::open(&db)
                            .map_err(|e| anyhow::anyhow!("opening local hub store: {e}"))?;
                        println!(
                            "{}",
                            store
                                .pairing_code()
                                .map_err(|e| anyhow::anyhow!("minting pairing code: {e}"))?
                        );
                    }
                    HubCmd::Adopt { node } => {
                        let store = LocalHubStore::open(&db)
                            .map_err(|e| anyhow::anyhow!("opening local hub store: {e}"))?;
                        // The owner is this machine's own verified Hive account, never something
                        // the joining machine supplies -- the whole point of doing this here.
                        let me = hub(&cfg)?.whoami().await?;
                        store
                            .set_node_owner(node, me.member_id)
                            .map_err(|e| anyhow::anyhow!("adopting node {node}: {e}"))?;
                        println!(
                            "node {node} now belongs to {} -- its agents can act on this hub",
                            me.display_name
                        );
                    }
                    HubCmd::Pair {
                        hub: hub_origin,
                        code,
                        name,
                    } => {
                        // Default to the name this node already goes by in Hive rather than
                        // adding a hostname dependency -- `whoami` is the authoritative source and
                        // the node must be paired with Hive for `hive bots` to work at all.
                        let name = match name {
                            Some(n) => n,
                            None => hub(&cfg)?
                                .whoami()
                                .await
                                .map(|me| me.display_name)
                                .unwrap_or_else(|_| "hive node".into()),
                        };
                        let credentials = RemoteLocalHub::pair(&hub_origin, &code, &name)
                            .await
                            .map_err(|e| anyhow::anyhow!("pairing with {hub_origin}: {e}"))?;
                        let path = local_hub_credentials_path();
                        write_local_hub_credentials(&path, &credentials)?;
                        println!(
                            "paired with {hub_origin} as node {} -- credentials saved to {}",
                            credentials.node_id,
                            path.display()
                        );
                        println!("now run: hive bots --hub {hub_origin} agent-list");
                    }
                    HubCmd::Schedule { cmd } => {
                        let store = LocalHubStore::open(&db)
                            .map_err(|e| anyhow::anyhow!("opening local hub store: {e}"))?;
                        // Schedules are a purely local-vault feature (no cross-node worker claim,
                        // runs in-process in the hub) -- resolve the owner straight from this
                        // node's own confirmed pairing, the same source `bots_owner()` uses for
                        // Paperclip, rather than a cloud Hive-account `whoami()` call. That call
                        // needs a paired HIVE_NODE_KEY, which a hub-serving machine's own HOME
                        // (e.g. the LaunchDaemon's) never has -- it only has the vault's own
                        // HIVE_VAULT_SELF_KEY. Requiring the former here would make `hive hub
                        // schedule` unusable on exactly the machine that runs the scheduler.
                        let schedule_owner = store
                            .resolve_owner()
                            .map_err(|e| anyhow::anyhow!("resolving account owner: {e}"))?;
                        match cmd {
                            ScheduleCmd::Create {
                                name,
                                agent,
                                every,
                                start_at,
                                text,
                            } => {
                                let text = text.join(" ");
                                if text.trim().is_empty() {
                                    anyhow::bail!("schedule text must not be empty");
                                }
                                let start_at_ts = start_at
                                    .map(|s| {
                                        chrono::DateTime::parse_from_rfc3339(&s)
                                            .map(|dt| dt.timestamp())
                                            .map_err(|e| {
                                                anyhow::anyhow!(
                                                    "--start-at must be an RFC3339 timestamp \
                                                     (e.g. 2026-09-28T13:00:00Z): {e}"
                                                )
                                            })
                                    })
                                    .transpose()?;
                                let id = store
                                    .schedules_create(
                                        schedule_owner,
                                        &name,
                                        agent,
                                        &text,
                                        every,
                                        start_at_ts,
                                    )
                                    .map_err(|e| anyhow::anyhow!("creating schedule: {e}"))?;
                                let when = start_at_ts
                                    .map(|t| {
                                        format!(
                                            ", first occurrence at {}",
                                            chrono::DateTime::from_timestamp(t, 0)
                                                .map(|dt| dt.to_rfc3339())
                                                .unwrap_or_else(|| t.to_string())
                                        )
                                    })
                                    .unwrap_or_default();
                                println!(
                                    "created schedule {id} \"{name}\" -- every {every}s to agent {agent}{when}"
                                );
                            }
                            ScheduleCmd::List => {
                                let schedules = store
                                    .schedules_list(schedule_owner)
                                    .map_err(|e| anyhow::anyhow!("listing schedules: {e}"))?;
                                if schedules.is_empty() {
                                    println!("no schedules yet — try `hive hub schedule create`");
                                }
                                for s in schedules {
                                    println!(
                                        "{}  {:<20}  agent={}  every={}s  {}{}",
                                        s.id,
                                        s.name,
                                        s.agent_id,
                                        s.every_secs,
                                        if s.enabled { "enabled" } else { "disabled" },
                                        if s.paused { ", paused" } else { "" },
                                    );
                                }
                            }
                            ScheduleCmd::Disable { id } => {
                                store
                                    .schedules_set_enabled(schedule_owner, id, false)
                                    .map_err(|e| anyhow::anyhow!("disabling schedule {id}: {e}"))?;
                                println!("disabled schedule {id}");
                            }
                            ScheduleCmd::Enable { id } => {
                                store
                                    .schedules_set_enabled(schedule_owner, id, true)
                                    .map_err(|e| anyhow::anyhow!("enabling schedule {id}: {e}"))?;
                                println!("enabled schedule {id}");
                            }
                        }
                    }
                    HubCmd::WebTool { cmd } => {
                        let store = LocalHubStore::open(&db)
                            .map_err(|e| anyhow::anyhow!("opening local hub store: {e}"))?;
                        match cmd {
                            WebToolCmd::GrantPostHost { agent, hosts } => {
                                let granted = store
                                    .bots_agent_web_post_hosts_grant_local(agent, hosts)
                                    .map_err(|e| anyhow::anyhow!("granting web-post hosts: {e}"))?;
                                println!(
                                    "agent {agent} may now web_post_json to: {}",
                                    granted.join(", ")
                                );
                            }
                            WebToolCmd::SetSecret { agent, host, token } => {
                                store
                                    .bots_agent_secret_set_local(agent, &host, &token)
                                    .map_err(|e| anyhow::anyhow!("setting secret: {e}"))?;
                                println!(
                                    "stored the secret for {host} on agent {agent} -- write \"{{{{SECRET}}}}\" in a web_post_json body to use it"
                                );
                            }
                        }
                    }

                    HubCmd::Budget { cmd } => {
                        let store = LocalHubStore::open(&db)
                            .map_err(|e| anyhow::anyhow!("opening local hub store: {e}"))?;
                        match cmd {
                            BudgetCmd::Set {
                                agent,
                                monthly_tokens,
                            } => {
                                store
                                    .bots_agent_budget_set_local(agent, monthly_tokens)
                                    .map_err(|e| anyhow::anyhow!("setting budget: {e}"))?;
                                println!("agent {agent} capped at {monthly_tokens} tokens/month");
                            }
                            BudgetCmd::Clear { agent } => {
                                store
                                    .bots_agent_budget_clear_local(agent)
                                    .map_err(|e| anyhow::anyhow!("clearing budget: {e}"))?;
                                println!("cleared agent {agent}'s monthly token cap");
                            }
                            BudgetCmd::Status { agent } => {
                                let status = store
                                    .bots_agent_budget_status_local(agent)
                                    .map_err(|e| anyhow::anyhow!("reading budget status: {e}"))?;
                                let cap = status
                                    .monthly_token_limit
                                    .map(|l| l.to_string())
                                    .unwrap_or_else(|| "none".to_string());
                                println!(
                                    "agent {agent} -- {}: {} prompt + {} completion = {} tokens (cap: {cap}){}",
                                    status.period,
                                    status.prompt_tokens,
                                    status.completion_tokens,
                                    status.total_tokens(),
                                    if status.over_budget() { "  [OVER BUDGET]" } else { "" }
                                );
                            }
                            BudgetCmd::List => {
                                let rows = store
                                    .bots_agent_budget_list_local()
                                    .map_err(|e| anyhow::anyhow!("listing budgets: {e}"))?;
                                if rows.is_empty() {
                                    println!(
                                        "no agents have a budget set or usage recorded this month"
                                    );
                                }
                                for status in rows {
                                    let cap = status
                                        .monthly_token_limit
                                        .map(|l| l.to_string())
                                        .unwrap_or_else(|| "none".to_string());
                                    println!(
                                        "{} -- {}: {} tokens (cap: {cap}){}",
                                        status.agent,
                                        status.period,
                                        status.total_tokens(),
                                        if status.over_budget() {
                                            "  [OVER BUDGET]"
                                        } else {
                                            ""
                                        }
                                    );
                                }
                            }
                        }
                    }
                }
            }
            #[cfg(not(feature = "bots"))]
            {
                let _ = cmd;
                anyhow::bail!("this build has no local hub support; rebuild with --features bots");
            }
        }
        Cmd::Bots { cmd, hub: hub_url } => {
            #[cfg(feature = "bots")]
            {
                use hive_core::bots::DeliveryStore;
                use hive_core::bots::{
                    AgentRuntimeKind, ConversationKind, DeliveryExecutor, LocalBotsTurnRunner,
                    LocalModelTurnRunner, MessageKind, MessagePage, NewAgentProfile,
                    NewConversation, NewMessage, Principal, StorageScope,
                };
                use hive_core::local_hub::{LocalHubStore, RemoteLocalHub};
                // One vault or another machine's -- every command below is written against the
                // `BotsBackend` surface, so nothing past this point knows which it got. The
                // remote case authenticates with the credentials `hive hub pair` saved; the hub
                // decides what this machine may touch, so a wrong `--hub` is refused rather than
                // silently working on the wrong data.
                // A node has TWO identities and mixing them is the whole trap here: its Hive
                // account node id (what `whoami` returns, used everywhere else in this CLI) and
                // its id inside the hub's vault (what `hive hub pair` issued). Host comparisons
                // happen in the vault's namespace, so against a remote hub every "which machine
                // am I" answer has to come from the credentials, not from `whoami`. Getting this
                // wrong does not error -- the delivery loop just reports the agent as assigned to
                // another computer, which is true and useless.
                // `host_node_id` is ALWAYS in the vault's namespace, for both arms -- there is
                // deliberately no fallback to the Hive account id. The local arm used to have
                // one, and it cost a day: on the hub machine it registered agents against an id
                // the vault has no row for, so the app's drain filed a "pinned to a computer
                // this vault does not know" notice on every message, which no amount of pairing
                // could clear. `LocalHubStore::self_node_id` asks the vault the same way the
                // desktop app does.
                let (store, host_node_id): (std::sync::Arc<dyn DeliveryStore>, uuid::Uuid) =
                    match hub_url.as_deref() {
                        Some(url) => {
                            let credentials = local_hub_credentials()?;
                            let node_id = credentials.node_id;
                            (
                                std::sync::Arc::new(
                                    RemoteLocalHub::new(url, credentials.raw_key).map_err(|e| {
                                        anyhow::anyhow!("connecting to hub {url}: {e}")
                                    })?,
                                ),
                                node_id,
                            )
                        }
                        None => {
                            let store = LocalHubStore::open(
                                config::path().with_file_name("vault-host.sqlite3"),
                            )
                            .map_err(|e| anyhow::anyhow!("opening local Bots store: {e}"))?;
                            let node_id = store.self_node_id().map_err(|e| {
                                anyhow::anyhow!("resolving this machine's id in its own vault: {e}")
                            })?;
                            (std::sync::Arc::new(store), node_id)
                        }
                    };
                match cmd {
                    BotsCmd::AgentRegister { name } => {
                        let me = hub(&cfg)?.whoami().await?;
                        let draft = NewAgentProfile {
                            owner: me.member_id,
                            name: name.unwrap_or_else(|| me.display_name.clone()),
                            runtime_kind: AgentRuntimeKind::Local,
                            // The vault's id for this machine, whichever vault that is. Every
                            // host check happens in the vault's namespace, so sending the Hive
                            // account id instead produces an agent that no machine appears to
                            // run -- the delivery loop then reports it as assigned to another
                            // computer, which is true, unhelpful, and took two live rounds to
                            // spot on a remote hub and a third on the hub machine itself.
                            preferred_host: Some(host_node_id),
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
                            agent.name,
                            agent.id,
                            agent
                                .preferred_host
                                .map(|h| h.to_string())
                                .unwrap_or_else(|| "-".to_string()),
                        );
                    }
                    BotsCmd::AgentArchive { agent } => {
                        let me = hub(&cfg)?.whoami().await?;
                        store
                            .agents_archive(me.member_id, agent)
                            .await
                            .map_err(|e| anyhow::anyhow!("archiving agent {agent}: {e}"))?;
                        println!("archived agent {agent}");
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
                        println!(
                            "room {} \"{}\" with {}",
                            room.id,
                            name,
                            chosen
                                .iter()
                                .map(|a| a.name.clone())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
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
                        println!(
                            "next: `hive bots say {} \"@{} hello\"`, with `hive bots work` running",
                            room.id,
                            chosen.first().map(|a| a.name.clone()).unwrap_or_default()
                        );
                    }
                    BotsCmd::Say {
                        conversation_id,
                        text,
                    } => {
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
                            .room_agents(Principal::User(me.member_id), room.id)
                            .await
                            .map_err(|e| anyhow::anyhow!("reading the room roster: {e}"))?;
                        let mentions = hive_core::bots::resolve_mentions(
                            &text,
                            &roster,
                            Principal::User(me.member_id),
                        );
                        if !mentions.unresolved.is_empty() {
                            println!(
                                "unrecognized name(s), nobody notified for them: {}",
                                mentions.unresolved.join(", ")
                            );
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
                            println!(
                                "posted (seq {}) -- no mentions, so nobody was woken",
                                sent.server_sequence
                            );
                        } else {
                            println!(
                                "posted (seq {}) -- woke {}; `hive bots read {}` for replies",
                                sent.server_sequence,
                                named.join(", "),
                                room.id
                            );
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
                            c.kind == ConversationKind::AgentDm && c.coordinator == Some(agent_id)
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
                                .map_err(|e| anyhow::anyhow!("creating DM conversation: {e}"))?,
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
                            let who = if m.kind == MessageKind::System {
                                "System".to_string()
                            } else {
                                match m.author {
                                    Principal::User(_) => "you".to_string(),
                                    Principal::Agent(id) => format!("agent {id}"),
                                }
                            };
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
                                        // Third and last place this identity is compared:
                                        // runner.rs re-checks `agent.preferred_host == self.host`
                                        // before running a turn, independently of the claim and
                                        // the executor's own filter. All three must agree, and
                                        // all three must be in the VAULT's namespace when the
                                        // vault belongs to another machine.
                                        host_node_id,
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
                                // The vault's id for this machine, remote hub or own vault.
                                // See the comment at the store selection above.
                                host_node_id,
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
                                        executor =
                                            executor.with_cloud_runner(std::sync::Arc::new(runner));
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
                let _ = (cmd, hub_url);
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

/// Where the credentials issued by `hive hub pair` live: beside `node.env` and the vault, in the
/// same private config directory, so one machine's hub identity travels with the rest of its node
/// configuration rather than landing in a working directory.
#[cfg(feature = "bots")]
fn local_hub_credentials_path() -> std::path::PathBuf {
    config::path().with_file_name("local-hub-credentials.json")
}

/// Written 0600 on Unix and created fresh each time: this is a bearer credential for another
/// machine's vault, so it must not be world-readable and must not be appended to an existing file.
#[cfg(feature = "bots")]
fn write_local_hub_credentials(
    path: &std::path::Path,
    credentials: &hive_core::local_hub::NodeCredentials,
) -> anyhow::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(path)?;
    f.write_all(&serde_json::to_vec(credentials)?)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(feature = "bots")]
fn local_hub_credentials() -> anyhow::Result<hive_core::local_hub::NodeCredentials> {
    let path = local_hub_credentials_path();
    let bytes = std::fs::read(&path).map_err(|e| {
        anyhow::anyhow!(
            "no hub credentials at {} ({e}) -- run `hive hub pair --hub <url> --code <code>` first, \
             with the code from `hive hub pair-code` on the hub machine",
            path.display()
        )
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|e| anyhow::anyhow!("hub credentials at {} are unreadable: {e}", path.display()))
}
