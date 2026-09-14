//! FFI wrapper for BYOK cloud chat from the Swift app (Jack, 2026-09-13: "get the BYOK to
//! swift" -- ADR-018 amendment decision 7 named a Claude BYOK package as a future `ChatProvider`
//! case for `ChatEngine.swift`, but it was never wired up; this is that wiring, minus a
//! dedicated Swift package -- it goes through the shared Rust core instead, same as every other
//! Hive network call). Same node-key-only constraint as `feedback.rs`: this app never holds a
//! member Supabase session, so `HubClient::interview_chat` goes through the `interview` Edge
//! Function's node-key fallback path (`hive_admin_verify_node_key` + `hive_admin_node_member`,
//! same resolution as feature requests/bug reports/hosted image generation) rather than the
//! member-JWT path the web app's `/new` page uses.
//!
//! Stateless by design, same as the web app: the whole conversation is resent every turn (no
//! server-side session to resume), so `ChatEngine.swift` owns the history and passes the full
//! transcript in on each call -- this method has no memory of its own between calls.
//!
//! Scope: always `mode: "chat"`, never `"plan"` -- turning a conversation into a Hive project
//! stays a web-only feature (see `interview`'s 2026-09-13 header note). No web search, no
//! streaming: one request, one reply, matching `ChatEngine.swift`'s existing on-device path's
//! shape (`session.respond(to:)` is not streaming either).

use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::hub::{ChatMemory as HubChatMemory, ChatTurn, HubClient};
use hive_core::nodeconfig;
use std::sync::Arc;

// Named `ByokChatTurn`, not `ChatMessage` -- `ChatEngine.swift` already defines its own
// `ChatMessage` (the on-screen bubble type, with an `id`/`Role` enum for its display needs), and
// both types would otherwise collide unqualified in the same Swift module.
#[derive(uniffi::Record, Clone)]
pub struct ByokChatTurn {
    /// "user" or "assistant" -- matches the `interview` Edge Function's `Msg` type exactly, so
    /// no translation happens on either side of the FFI boundary.
    pub role: String,
    pub content: String,
}

#[derive(uniffi::Record, Clone)]
pub struct ChatReply {
    pub reply: String,
    /// e.g. "your anthropic key" -- a provenance hint for the UI, not load-bearing.
    pub brain: Option<String>,
}

/// The member's persistent chat memory (2026-09-13, Hermes-agent survey -- see
/// `supabase/migrations/20260913010000_chat_memory.sql` for the full rationale). Read-only from
/// this FFI surface: only the `interview` Edge Function's background pass writes it, on the
/// member's own BYOK key -- `ChatEngine.swift`'s on-device path fetches this once per session to
/// pick up what BYOK sessions have taught the assistant, without ever writing to it itself.
#[derive(uniffi::Record, Clone)]
pub struct ChatMemory {
    pub memory_md: String,
    pub user_md: String,
}

impl From<HubChatMemory> for ChatMemory {
    fn from(m: HubChatMemory) -> Self {
        Self {
            memory_md: m.memory_md,
            user_md: m.user_md,
        }
    }
}

#[uniffi::export]
impl HiveNode {
    /// `history` is the full conversation so far, ending with the new user turn -- same
    /// stateless shape as the web app's `/new` page. Returns a friendly error (not raw JSON) when
    /// no BYOK key is on file, since that's the one failure mode a person will actually hit.
    pub async fn send_byok_chat(
        self: Arc<Self>,
        history: Vec<ByokChatTurn>,
    ) -> Result<ChatReply, HiveError> {
        self.log("info", "sending a chat turn via your API key").await;
        let this = self.clone();
        let r = RUNTIME
            .spawn(async move {
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let key = cfg
                    .node_key
                    .clone()
                    .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
                let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
                let turns: Vec<ChatTurn> = history
                    .into_iter()
                    .map(|m| ChatTurn {
                        role: m.role,
                        content: m.content,
                    })
                    .collect();
                let result = hub.interview_chat(&turns).await.map_err(|e| {
                    let msg = e.to_string();
                    if msg.contains("no_byo_key") {
                        HiveError::Failed(
                            "add an API key (Anthropic, OpenAI, or Nous) in Settings on the web app to chat with a frontier model, or switch back to On-device".into(),
                        )
                    } else {
                        HiveError::from(e)
                    }
                })?;
                Ok(ChatReply {
                    reply: result.reply,
                    brain: result.brain,
                })
            })
            .await
            .map_err(|e| HiveError::Failed(format!("send_byok_chat task panicked: {e}")))?;
        match &r {
            Ok(_) => this.log("ok", "chat reply received").await,
            Err(e) => this.log("error", format!("chat turn failed: {e}")).await,
        }
        r
    }

    /// Additive twin of `send_byok_chat` (2026-09-13, the chat composer's two-stage
    /// provider-then-model picker): same stateless full-transcript resend, but with an explicit
    /// `provider` ("anthropic" | "openai" | "nous") the member picked, and an optional `model`
    /// overriding that provider's saved preference for this turn only. Empty `model` means "use
    /// the provider's saved preferred_model, or the function's default" -- same fallback
    /// `byok_keys.rs::set_byok_key_model` already establishes for the web app's picker, just
    /// applied per-turn instead of being saved. Fails the same friendly way as `send_byok_chat`
    /// when the picked provider has no key on file (`provider_key_not_configured` from the Edge
    /// Function, translated below) -- the picker itself should prevent this by only listing
    /// providers `byokKeysStatus()` says are configured, but the network call is the source of
    /// truth if that state is ever stale.
    pub async fn send_byok_chat_with(
        self: Arc<Self>,
        history: Vec<ByokChatTurn>,
        provider: String,
        model: Option<String>,
    ) -> Result<ChatReply, HiveError> {
        self.log("info", format!("sending a chat turn via your {provider} key")).await;
        let this = self.clone();
        let r = RUNTIME
            .spawn(async move {
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let key = cfg
                    .node_key
                    .clone()
                    .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
                let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
                let turns: Vec<ChatTurn> = history
                    .into_iter()
                    .map(|m| ChatTurn {
                        role: m.role,
                        content: m.content,
                    })
                    .collect();
                let model = model.filter(|m| !m.trim().is_empty());
                let result = hub
                    .interview_chat_with(Some(&provider), model.as_deref(), &turns)
                    .await
                    .map_err(|e| {
                        let msg = e.to_string();
                        if msg.contains("no_byo_key") || msg.contains("provider_key_not_configured") {
                            HiveError::Failed(format!(
                                "no {provider} key on file -- add one in Settings, or pick a different provider"
                            ))
                        } else {
                            HiveError::from(e)
                        }
                    })?;
                Ok(ChatReply {
                    reply: result.reply,
                    brain: result.brain,
                })
            })
            .await
            .map_err(|e| HiveError::Failed(format!("send_byok_chat_with task panicked: {e}")))?;
        match &r {
            Ok(_) => this.log("ok", "chat reply received").await,
            Err(e) => this.log("error", format!("chat turn failed: {e}")).await,
        }
        r
    }

    /// Fetches this node's owning member's persistent chat memory (see `ChatMemory`'s doc
    /// comment). `ChatEngine.swift`'s on-device path calls this once per session and folds it
    /// into the model's instructions, the same way the `interview` Edge Function folds it into
    /// the system prompt for BYOK sessions -- so on-device chat has continuity with what BYOK
    /// sessions have learned, even though it never writes to memory itself. Errors (unpaired,
    /// hub unreachable) are swallowed by the caller the same way `kanbanCloudProjects` does --
    /// this is a nice-to-have, not something that should block opening the Chat tab.
    pub async fn get_chat_memory(self: Arc<Self>) -> Result<ChatMemory, HiveError> {
        let cfg = nodeconfig::load().map_err(HiveError::from)?;
        let key = cfg
            .node_key
            .clone()
            .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
        let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
        let result = RUNTIME
            .spawn(async move { hub.chat_memory_get().await })
            .await
            .map_err(|e| HiveError::Failed(format!("get_chat_memory task panicked: {e}")))?
            .map_err(HiveError::from)?;
        Ok(result.into())
    }
}
