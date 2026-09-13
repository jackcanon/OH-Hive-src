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
use hive_core::hub::{ChatTurn, HubClient};
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
}
