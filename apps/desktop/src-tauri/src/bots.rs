//! Team-chat Bots commands for the Tauri desktop app (ADR-035 C1, 2026-09-15). Thin wrappers
//! around the same `LocalHubStore`/`BotsService` the `hive` CLI's `bots` subcommand and the
//! Swift bridge (`crates/ohhive-ffi/src/bots.rs`) already use -- all three read/write the same
//! `vault-host.sqlite3`, so a message sent from any one of them is visible, and repliable, from
//! the others. Human-driven team/project rooms share the core mention resolver. Replies
//! still create no further deliveries. Remote-primary selection is native-only; this shell
//! refuses to open divergent local history when a primary is selected.
//!
//! Unlike the CLI (`hive bots work`, a separate long-running process) or the Swift bridge
//! (which explicitly does *not* start a model runner -- see Sif's FFI handoff), this app owns
//! its own delivery drain loop (`spawn_drain_loop`, started once from `lib.rs`'s `.setup()`):
//! opening the app is enough to have its own locally-hosted agents actually reply, no second
//! terminal needed. It only ever touches this node's own local agents (`preferred_host ==
//! this node`), so running it alongside a CLI `hive bots work` for the same account is safe --
//! `bots_delivery_claim`'s lease-generation fencing means at most one of them wins a given
//! delivery, never both.
//!
//! Not compiler-verified when written -- no Rust toolchain in the sandbox that wrote it. Needs
//! a real `cargo build` (this app's `bots`/`local-hub` features are newly added in the same
//! change) before it's trusted.

use crate::model_pref;
use hive_core::bots::{
    AgentRuntimeKind, BotsService, ConversationKind, DeliveryExecutor, LocalBotsTurnRunner,
    LocalModelTurnRunner, MessageKind, MessagePage, NewAgentProfile, NewConversation, NewMessage,
    Principal, StorageScope,
};
use hive_core::hub::HubClient;
use hive_core::local_hub::LocalHubStore;
use hive_core::nodeconfig;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const DRAIN_POLL: Duration = Duration::from_secs(5);

fn open_store() -> Result<Arc<LocalHubStore>, String> {
    // This shell has not yet adopted the Swift remote-primary adapter. Never fork history.
    match std::fs::symlink_metadata(nodeconfig::path().with_file_name("private-primary.json")) {
        Ok(_) => return Err("Use the native app for your selected private primary; web desktop cannot connect to it yet".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(_) => return Err("Cannot read private primary selection".into()),
    }

    LocalHubStore::open(nodeconfig::path().with_file_name("vault-host.sqlite3"))
        .map(Arc::new)
        .map_err(|e| format!("opening local Bots store: {e}"))
}

/// This account's identity plus this node's own id, straight from the hub -- never taken from
/// the frontend, same "author identity is never UI-supplied" rule the Swift bridge documents.
async fn whoami() -> Result<hive_core::hub::WhoAmI, String> {
    let cfg = nodeconfig::load().map_err(|e| e.to_string())?;
    let key = cfg
        .node_key
        .clone()
        .ok_or_else(|| "pair this Mac before opening team chat".to_string())?;
    HubClient::new(&cfg.hub_url, &cfg.anon_key, key)
        .whoami()
        .await
        .map_err(|e| e.to_string())
}

fn parse_id(s: &str) -> Result<Uuid, String> {
    Uuid::parse_str(s).map_err(|_| "invalid id".to_string())
}

#[derive(Serialize)]
pub struct BotsAgentView {
    pub id: String,
    pub name: String,
    pub preferred_host: Option<String>,
    pub archived: bool,
}
impl From<hive_core::bots::AgentProfile> for BotsAgentView {
    fn from(a: hive_core::bots::AgentProfile) -> Self {
        Self {
            id: a.id.to_string(),
            name: a.name,
            preferred_host: a.preferred_host.map(|v| v.to_string()),
            archived: a.archived,
        }
    }
}

#[derive(Serialize)]
pub struct BotsConversationView {
    pub title: Option<String>,
    pub kind: ConversationKind,
    pub project_id: Option<String>,
    pub id: String,
    pub coordinator: Option<String>,
    pub policy_revision: u32,
}
impl From<hive_core::bots::Conversation> for BotsConversationView {
    fn from(c: hive_core::bots::Conversation) -> Self {
        Self {
            title: c.title, kind: c.kind, project_id: c.project_id.map(|p| p.to_string()),
            id: c.id.to_string(),
            coordinator: c.coordinator.map(|v| v.to_string()),
            policy_revision: c.policy_revision,
        }
    }
}

#[derive(Serialize)]
pub struct BotsMessageView {
    pub id: String,
    pub server_sequence: u64,
    pub author_id: String,
    pub author: String, // "you" or "agent"
    pub body: Option<String>,
    pub created_at: String,
}
impl From<hive_core::bots::Message> for BotsMessageView {
    fn from(m: hive_core::bots::Message) -> Self {
        let author = match m.author {
            Principal::User(_) => "you".to_string(),
            Principal::Agent(_) => "agent".to_string(),
        };
        Self {
            id: m.id.to_string(),
            server_sequence: m.server_sequence,
            author_id: match m.author { Principal::User(id) | Principal::Agent(id) => id.to_string() },
            author,
            body: m.body,
            created_at: m.created_at.to_rfc3339(),
        }
    }
}

/// List every Bots agent this account owns, on any node -- not just this one.
#[tauri::command]
pub async fn bots_agents_list() -> Result<Vec<BotsAgentView>, String> {
    let me = whoami().await?;
    let store = open_store()?;
    store
        .agents_list(me.member_id)
        .await
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(|e| e.to_string())
}

/// Register this Mac as a local agent. Safe to call again with a different name to add a
/// second agent hosted here; there's no dedup on node id yet (matches the CLI).
#[tauri::command]
pub async fn bots_agent_register(name: Option<String>) -> Result<BotsAgentView, String> {
    let me = whoami().await?;
    let store = open_store()?;
    store
        .agents_create(NewAgentProfile {
            owner: me.member_id,
            name: name.unwrap_or(me.display_name),
            runtime_kind: AgentRuntimeKind::Local,
            preferred_host: Some(me.node_id),
            capability_policy_ref: "default".to_string(),
            provider_account_ref: None,
            memory_namespace: format!("agent:{}", me.node_id),
        })
        .await
        .map(Into::into)
        .map_err(|e| e.to_string())
}

/// Auto-provisions a Bots agent for every BYOK provider (Anthropic, Nous) this member has a
/// key on file for in Settings -- mirrors `ohhive-ffi`'s `BotsSession::ensure_provider_agents`
/// (Swift side); kept as a parallel native implementation here rather than a shared helper
/// because the Tauri app talks to `hive-core` directly, no FFI/UniFFI boundary in between.
/// Idempotent: skips any provider that already has a non-archived agent of the matching
/// runtime kind. Creates the agent identity only -- no cloud turn runner exists yet to answer
/// as one of these agents (see docs/LOKI-BOTS-C2-GROUP-AND-ADAPTER-AGENTS-PLAN-2026-09-15.md).
#[tauri::command]
pub async fn bots_ensure_provider_agents() -> Result<Vec<BotsAgentView>, String> {
    let me = whoami().await?;
    let cfg = nodeconfig::load().map_err(|e| e.to_string())?;
    let key = cfg
        .node_key
        .clone()
        .ok_or_else(|| "pair this Mac before opening team chat".to_string())?;
    let status = HubClient::new(&cfg.hub_url, &cfg.anon_key, key)
        .member_key_status()
        .await
        .map_err(|e| e.to_string())?;
    let wanted: Vec<(AgentRuntimeKind, &str, bool)> = vec![
        (AgentRuntimeKind::AnthropicByok, "Claude", status.anthropic.is_some()),
        (AgentRuntimeKind::NousByok, "Nous", status.nous.is_some()),
    ];
    let store = open_store()?;
    let existing = store.agents_list(me.member_id).await.map_err(|e| e.to_string())?;
    let mut created = Vec::new();
    for (kind, default_name, has_key) in wanted {
        if !has_key {
            continue;
        }
        if existing.iter().any(|a| a.runtime_kind == kind && !a.archived) {
            continue;
        }
        let agent = store
            .agents_create(NewAgentProfile {
                owner: me.member_id,
                name: default_name.to_string(),
                runtime_kind: kind,
                preferred_host: None,
                capability_policy_ref: "default".to_string(),
                provider_account_ref: None,
                memory_namespace: format!("agent:{}", Uuid::new_v4()),
            })
            .await
            .map_err(|e| e.to_string())?;
        created.push(agent.into());
    }
    Ok(created)
}

/// Find or create the one DM conversation with this agent -- the UI never has to think about
/// conversation ids up front, just the agent it wants to talk to.
#[tauri::command]
pub async fn bots_dm_open(agent_id: String) -> Result<BotsConversationView, String> {
    let agent_id = parse_id(&agent_id)?;
    let me = whoami().await?;
    let store = open_store()?;
    if !store.bots_agents_list(me.member_id).map_err(|e| e.to_string())?.iter().any(|a| a.id == agent_id && !a.archived) { return Err("Agent not available to this account".into()); }
    let existing = store
        .conversations_list(Principal::User(me.member_id))
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.kind == ConversationKind::AgentDm && c.coordinator == Some(agent_id));
    if let Some(c) = existing {
        return Ok(c.into());
    }
    store
        .conversations_create(NewConversation {
                    title: None,
            owner: me.member_id,
            kind: ConversationKind::AgentDm,
            project_id: None,
            coordinator: Some(agent_id),
            storage_scope: StorageScope::LocalOnly,
        })
        .await
        .map(Into::into)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn bots_rooms_list() -> Result<Vec<BotsConversationView>, String> {
    let me = whoami().await?; let store = open_store()?;
    Ok(store.bots_conversations_list(Principal::User(me.member_id)).map_err(|e| e.to_string())?.into_iter().filter(|c| c.kind != ConversationKind::AgentDm).map(Into::into).collect())
}
#[tauri::command]
pub async fn bots_room_create(title: String, agent_ids: Vec<String>, project_id: Option<String>) -> Result<BotsConversationView, String> {
    if title.trim().is_empty() || title.len() > 200 || agent_ids.is_empty() || agent_ids.len() > 16 { return Err("Name the room and choose 1–16 agents".into()); }
    let me = whoami().await?; let store = open_store()?;
    let ids = agent_ids.iter().map(|id| parse_id(id)).collect::<Result<Vec<_>, _>>()?;
    let owned = store.bots_agents_list(me.member_id).map_err(|e| e.to_string())?;
    if ids.iter().any(|id| !owned.iter().any(|a| a.id == *id && !a.archived)) { return Err("Room agents must belong to this account".into()); }
    let project_id = project_id.as_deref().map(parse_id).transpose()?;
    let room = store.bots_conversations_create(NewConversation { title: Some(title.trim().into()), owner: me.member_id, kind: if project_id.is_some() { ConversationKind::Project } else { ConversationKind::Team }, project_id, coordinator: None, storage_scope: StorageScope::LocalOnly }).map_err(|e| e.to_string())?;
    for id in ids { store.bots_conversations_join(Principal::Agent(id), room.id).map_err(|e| e.to_string())?; }
    Ok(room.into())
}
#[derive(Serialize)]
pub struct MentionsView { pub recipient_ids: Vec<String>, pub unresolved: Vec<String> }
#[tauri::command]
pub async fn bots_mentions_resolve(conversation_id: String, text: String) -> Result<MentionsView, String> {
    if text.len() > 65536 { return Err("Message too long".into()); }
    let me = whoami().await?; let store = open_store()?;
    let roster = store.bots_room_agents(Principal::User(me.member_id), parse_id(&conversation_id)?).map_err(|e| e.to_string())?;
    let mentions = hive_core::bots::resolve_mentions(&text, &roster, Principal::User(me.member_id));
    Ok(MentionsView { recipient_ids: mentions.recipients.iter().map(ToString::to_string).collect(), unresolved: mentions.unresolved })
}
#[tauri::command]
pub async fn bots_chat_send(conversation_id: String, recipient_ids: Vec<String>, expected_policy_revision: u32, text: String, request_id: String) -> Result<BotsMessageView, String> {
    if text.trim().is_empty() || text.len() > 65536 || recipient_ids.len() > 16 || request_id.is_empty() || request_id.len() > 200 { return Err("Invalid message or request ID".into()); }
    let me = whoami().await?; let store = open_store()?; let cid = parse_id(&conversation_id)?;
    let room = store.bots_conversations_list(Principal::User(me.member_id)).map_err(|e| e.to_string())?.into_iter().find(|c| c.id == cid && c.owner == me.member_id && c.storage_scope == StorageScope::LocalOnly).ok_or("Conversation not available")?;
    let recipients = recipient_ids.iter().map(|s| parse_id(s)).collect::<Result<Vec<_>, _>>()?;
    if room.kind == ConversationKind::AgentDm && (recipients.len() != 1 || recipients.first().copied() != room.coordinator) { return Err("DM recipient must be its coordinator".into()); }
    store.bots_message_send(Principal::User(me.member_id), cid, request_id, expected_policy_revision, recipients, NewMessage { thread_root: None, kind: MessageKind::Text, body: Some(text), attachment_refs: vec![], task_ref: None, turn_ref: None, source_event_ref: None }).map(Into::into).map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn bots_projects_list() -> Result<serde_json::Value, String> {
    let cfg = nodeconfig::load().map_err(|e| e.to_string())?;
    let key = cfg.node_key.clone().ok_or("Connect to your community Hive first")?;
    HubClient::new(&cfg.hub_url, &cfg.anon_key, key).node_projects_overview().await.map_err(|e| e.to_string())
}

/// Messages in a conversation, oldest first. Pass the last sequence you already have as
/// `after` to poll for just what's new.
#[tauri::command]
pub async fn bots_messages_list(
    conversation_id: String,
    after: Option<u64>,
) -> Result<Vec<BotsMessageView>, String> {
    let conversation_id = parse_id(&conversation_id)?;
    let me = whoami().await?;
    let store = open_store()?;
    store
        .messages_list(
            Principal::User(me.member_id),
            conversation_id,
            MessagePage {
                before: None,
                after,
                limit: 200,
            },
        )
        .await
        .map(|v| v.into_iter().map(Into::into).collect())
        .map_err(|e| e.to_string())
}

/// Send a DM to `agent_id` over `conversation_id` (from `bots_dm_open`). `expected_policy_
/// revision` is that conversation's `policy_revision`, as last seen by the caller -- a stale
/// value (someone else changed the conversation) is rejected rather than silently overwritten.
#[tauri::command]
pub async fn bots_dm_send(
    conversation_id: String,
    agent_id: String,
    expected_policy_revision: u32,
    text: String,
) -> Result<BotsMessageView, String> {
    if text.trim().is_empty() {
        return Err("message text is required".to_string());
    }
    let conversation_id = parse_id(&conversation_id)?;
    let agent_id = parse_id(&agent_id)?;
    let me = whoami().await?;
    let store = open_store()?;
    store
        .message_send(
            Principal::User(me.member_id),
            conversation_id,
            Uuid::new_v4().to_string(),
            expected_policy_revision,
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
        .map(Into::into)
        .map_err(|e| e.to_string())
}

/// Runs for the life of the app: every `DRAIN_POLL`, drains this node's own locally-hosted
/// agents' pending deliveries through a real local model turn. Skips a pass (rather than
/// erroring the app) whenever the node isn't paired yet, no model is configured, or the
/// loopback runner can't be built -- all recoverable states the user fixes from Setup/Settings,
/// not something a background task should surface as a crash or a banner of its own.
pub async fn spawn_drain_loop() {
    loop {
        tokio::time::sleep(DRAIN_POLL).await;
        let Ok(me) = whoami().await else { continue };
        let Some(model) = model_pref() else { continue };
        let cfg = match nodeconfig::load() {
            Ok(c) => c,
            Err(_) => continue,
        };
        let runner = match LocalModelTurnRunner::loopback(me.node_id, model, &cfg.llama_url) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let Ok(store) = open_store() else { continue };
        let runner: Arc<dyn LocalBotsTurnRunner> = Arc::new(runner);
        let executor = DeliveryExecutor::new(store, runner, me.node_id, me.member_id);
        executor.drain_once().await;
    }
}
