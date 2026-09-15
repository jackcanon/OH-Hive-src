//! Shared agent-loop vocabulary between the coding agent (`crate::coder`, ADR-024) and the
//! desktop/computer-use session (`crate::desktop`, ADR-029).
//!
//! Extracted from `crate::coder` (2026-09-15), per ADR-029's own phase-1 build sequence's first
//! bullet -- "shared multimodal messages and executor interface" -- which `crate::desktop`'s own
//! `ContentBlock` doc already anticipated ("Versioned multimodal contract for the later shared
//! session-loop integration"). Nothing here assumes what the tools are, how they're executed, or
//! what trust boundary they run inside: `crate::coder`'s four unsandboxed filesystem/process
//! tools and a future desktop executor's GUI actions are both just callers of
//! [`CodeBrain::next_turn`], and both build [`BrainMessage`]s the same way.
//!
//! **What this slice does, precisely:** moves the brain seam (`BrainRole`/`BrainToolCall`/
//! `BrainMessage`/`ToolSpec`/`BrainTurn`/`CodeBrainError`/`CodeBrain`) and `ContentBlock` out of
//! `crate::coder`/`crate::desktop` into one shared module, and widens `BrainMessage::content`
//! from `Option<String>` to `Vec<ContentBlock>` so a desktop turn can eventually carry a
//! screenshot. `crate::coder`'s two concrete brains (`LocalBrain`/`CloudBrain`) stay exactly
//! where they are -- they're coding-specific wire adapters (Ollama tool-calling, the
//! `code-brain-turn` Edge Function), neither of which understands images yet, so both now call
//! [`BrainMessage::text`] to flatten multimodal content back to the plain string their wire
//! formats expect. **What this slice does not do:** it does not make `crate::desktop`'s
//! `AnthropicDesktop` provider implement `CodeBrain`, does not wire a desktop session into
//! `run_session`'s loop, and does not touch `worker.rs`'s claim/dispatch path -- that's the
//! larger "executor wiring" work `crate::desktop::mod`'s own doc and Sif's session-controls
//! handoff both flag as still open, not attempted here.

use serde::{Deserialize, Serialize};

/// One role in an agent conversation -- the same four roles every tool-calling chat API
/// (OpenAI, Anthropic, Ollama) uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainRole {
    System,
    User,
    Assistant,
    Tool,
}

/// One tool call a brain asked for. `arguments` is a parsed JSON object (not the wire-format
/// JSON-encoded string some APIs use -- each [`CodeBrain`] implementation is responsible for its
/// own wire format's encoding/decoding; this is the one shape every implementation converts
/// to/from).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainToolCall {
    /// Stable id correlating this call to the [`BrainMessage::tool_result`] that answers it.
    /// Synthesized by the brain implementation if its own wire format doesn't provide one.
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Versioned multimodal content block. Moved here (2026-09-15) from `crate::desktop`, whose own
/// doc comment already named this as "the later shared session-loop integration." Images carry
/// bounded bytes, never a path or URL to fetch -- a provider/native adapter must still decode and
/// validate the PNG itself; [`ContentBlock::validate_envelope`] only checks the envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Png {
        bytes: Vec<u8>,
        width: u32,
        height: u32,
    },
}

impl ContentBlock {
    /// Envelope bounds only; native/provider adapters must also decode and validate the PNG.
    pub fn validate_envelope(&self) -> bool {
        match self {
            Self::Text { text } => text.len() <= 64 * 1024,
            Self::Png {
                bytes,
                width,
                height,
            } => {
                *width > 0
                    && *height > 0
                    && *width <= 4096
                    && *height <= 4096
                    && bytes.len() <= 8 * 1024 * 1024
                    && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            }
        }
    }
}

/// One message in the running agent conversation [`CodeBrain::next_turn`] is asked to continue.
/// This is the *only* vocabulary a second `CodeBrain` implementation needs to match -- nothing
/// about this shape assumes anything about how a turn is produced (a local HTTP call to Ollama,
/// a cloud provider round-trip, or -- once wired -- a desktop turn carrying a screenshot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainMessage {
    pub role: BrainRole,
    /// Multimodal content blocks. Widened (2026-09-15) from `Option<String>` so a desktop turn
    /// can eventually carry a screenshot alongside or instead of text -- an empty vec is exactly
    /// what the old `None` meant (a pure tool-calls assistant turn has no content at all). A
    /// caller that only ever deals in plain text should use [`BrainMessage::text`] rather than
    /// matching on this field directly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ContentBlock>,
    /// Set only on an `Assistant` message that made tool calls; echoed back on every later turn
    /// (both cloud and local tool-calling protocols require the exact prior assistant
    /// `tool_calls` to still be present in the next request).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<BrainToolCall>,
    /// Set only on a `Tool` message: which call (by [`BrainToolCall::id`]) this is the result of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl BrainMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: BrainRole::System,
            content: vec![ContentBlock::Text {
                text: content.into(),
            }],
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: BrainRole::User,
            content: vec![ContentBlock::Text {
                text: content.into(),
            }],
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    /// A user turn carrying more than one content block (e.g. a screenshot plus a short
    /// instruction) -- not yet produced by anything in this codebase; added so a future desktop
    /// session has the constructor ready rather than reaching into `content` directly.
    pub fn user_multimodal(blocks: Vec<ContentBlock>) -> Self {
        Self {
            role: BrainRole::User,
            content: blocks,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn assistant_tool_calls(tool_calls: Vec<BrainToolCall>) -> Self {
        Self {
            role: BrainRole::Assistant,
            content: Vec::new(),
            tool_calls,
            tool_call_id: None,
        }
    }
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: BrainRole::Tool,
            content: vec![ContentBlock::Text {
                text: content.into(),
            }],
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }
    /// Flattens this message's text content blocks into a plain string, the same shape every
    /// wire format that doesn't understand images yet (`coder::LocalBrain`/`CloudBrain`, both
    /// still text-only) expects. `Png` blocks contribute nothing here -- a caller that actually
    /// wants to send an image must handle `content` directly, not this accessor. An empty
    /// `content` (a pure tool-calls assistant turn) returns `None`, matching what the old
    /// `Option<String>` field meant before this type widened.
    pub fn text(&self) -> Option<String> {
        if self.content.is_empty() {
            return None;
        }
        Some(
            self.content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    ContentBlock::Png { .. } => None,
                })
                .collect::<Vec<_>>()
                .join(""),
        )
    }
}

/// One tool's JSON-schema description, in the (now near-universal) OpenAI function-calling
/// shape. Every [`CodeBrain`] implementation is expected to translate this into its own wire
/// format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// What a brain decided to do with the conversation it was handed.
#[derive(Debug, Clone)]
pub enum BrainTurn {
    /// The brain is done calling tools and this is its final (or intermediate-but-textual)
    /// reply. A loop driving this trait treats any `Text` turn as the session's answer and stops
    /// looping -- a brain that wants to keep working must call a tool, not narrate in prose.
    Text(String),
    /// The brain wants one or more tools run before it says anything else. Never empty -- an
    /// implementation that gets an empty `tool_calls` array from its own API should treat that
    /// as [`BrainTurn::Text`] with whatever text (possibly empty) came with it instead.
    ToolCalls(Vec<BrainToolCall>),
}

#[derive(thiserror::Error, Debug)]
pub enum CodeBrainError {
    #[error("brain backend error: {0}")]
    Backend(String),
}

/// The one seam between an agentic loop and however a "what should I do next" turn actually gets
/// answered. Implement this and the loop driving it never needs to change: it only ever calls
/// [`CodeBrain::next_turn`] with the conversation so far and the fixed tool schema, and only ever
/// inspects the [`BrainTurn`] it gets back -- it has no idea whether that turn came from a local
/// HTTP call to Ollama, a round-trip to a cloud provider, or a future desktop-shaped brain, and
/// it must never be given a reason to care.
#[async_trait::async_trait]
pub trait CodeBrain: Send + Sync {
    /// Given the conversation so far (system prompt, the task, every prior assistant/tool turn)
    /// and the tool schema available this session, decide what happens next. Called once per
    /// loop turn; implementations should treat each call as stateless (all state the brain needs
    /// is in `messages`) since a driving loop doesn't guarantee the same `CodeBrain` instance is
    /// reused across turns any more than it's required to be.
    async fn next_turn(
        &self,
        messages: &[BrainMessage],
        tools: &[ToolSpec],
    ) -> Result<BrainTurn, CodeBrainError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_accessor_flattens_text_blocks_and_ignores_png() {
        let m = BrainMessage::user_multimodal(vec![
            ContentBlock::Text {
                text: "before ".into(),
            },
            ContentBlock::Png {
                bytes: b"\x89PNG\r\n\x1a\nrest".to_vec(),
                width: 10,
                height: 10,
            },
            ContentBlock::Text {
                text: "after".into(),
            },
        ]);
        assert_eq!(m.text(), Some("before after".to_string()));
    }

    #[test]
    fn text_accessor_returns_none_for_empty_content() {
        let m = BrainMessage::assistant_tool_calls(vec![]);
        assert_eq!(m.text(), None);
    }

    #[test]
    fn plain_constructors_round_trip_through_text() {
        let m = BrainMessage::system("hello");
        assert_eq!(m.text(), Some("hello".to_string()));
        let t = BrainMessage::tool_result("call-1", "result body");
        assert_eq!(t.text(), Some("result body".to_string()));
        assert_eq!(t.tool_call_id.as_deref(), Some("call-1"));
    }

    #[test]
    fn content_block_validate_envelope_matches_previous_desktop_behavior() {
        assert!(ContentBlock::Text {
            text: "ok".into()
        }
        .validate_envelope());
        assert!(!ContentBlock::Png {
            bytes: b"not a png".to_vec(),
            width: 1,
            height: 1,
        }
        .validate_envelope());
    }
}
