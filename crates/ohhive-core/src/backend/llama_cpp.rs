//! llama.cpp adapter (ADR-003 D16/D61).
//!
//! Talks to any server that speaks the OpenAI-compatible `/v1/chat/completions`
//! SSE protocol: `llama-server` from llama.cpp, or Ollama (which wraps
//! llama.cpp and is the "convenience wrapper" ADR-003 allows). Process
//! management (spawning `llama-server` with a GGUF) is a separate concern that
//! lives in the node app; this adapter only needs a base URL.
//!
//! Metering (ADR-002 §12): token counts come from the server's own `usage`
//! object when it sends one. If the server never reports usage, we fall back
//! to counting output deltas (≈1 token each for llama.cpp) and mark it in
//! `Usage` via a tracing warning — the coordinator treats unreported usage as
//! lower-trust and eligible for spot-check replay.

use super::{Backend, BackendError, Chunk, ChunkStream};
use crate::capability::{Capabilities, GpuVendor, Hardware, Modality, ModelRef, ToolsLevel};
use crate::job::Job;
use crate::ledger::Usage;
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Instant;

pub struct LlamaCppBackend {
    /// e.g. `http://127.0.0.1:8080` (llama-server) or `http://127.0.0.1:11434` (Ollama).
    pub base_url: String,
    client: reqwest::Client,
}

impl LlamaCppBackend {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    async fn list_models(&self) -> Result<Vec<String>, BackendError> {
        #[derive(Deserialize)]
        struct Models {
            data: Vec<ModelEntry>,
        }
        #[derive(Deserialize)]
        struct ModelEntry {
            id: String,
        }
        let r = self
            .client
            .get(format!("{}/v1/models", self.base_url))
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?
            .error_for_status()
            .map_err(|e| BackendError::Unavailable(e.to_string()))?
            .json::<Models>()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        Ok(r.data.into_iter().map(|m| m.id).collect())
    }
}

#[derive(Deserialize)]
struct SseChunk {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<SseUsage>,
}
#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}
#[derive(Deserialize, Default)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
}
#[derive(Deserialize)]
struct SseUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

#[async_trait]
impl Backend for LlamaCppBackend {
    fn name(&self) -> &'static str {
        "llama_cpp"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn capabilities(&self) -> Result<Capabilities, BackendError> {
        let models = self.list_models().await?;
        Ok(Capabilities {
            // Hardware probe is the node app's job (ADR-010 step 2); the adapter
            // only knows what the server exposes. Placeholders until probe lands.
            hardware: Hardware {
                cpu_model: String::new(),
                cpu_cores: 0,
                ram_bytes: 0,
                ram_free_bytes: None,
                gpu_vendor: GpuVendor::None,
                gpu_model: None,
                vram_bytes: None,
                vram_free_bytes: None,
                disk_free_bytes: 0,
                upload_mbps: None,
                download_mbps: None,
            },
            modalities: vec![Modality::Text, Modality::Code],
            models: models
                .into_iter()
                .map(|id| ModelRef {
                    id,
                    modality: Modality::Text,
                    backend: "llama_cpp".into(),
                })
                .collect(),
            allow_internet: false,
            tools_level: ToolsLevel::SandboxedTools,
            storage_gb_offered: None,
            shard_capable: None,
        })
    }

    async fn run<'a>(&'a self, job: &'a Job) -> Result<ChunkStream<'a>, BackendError> {
        let prompt = job
            .input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BackendError::Rejected("input.prompt missing".into()))?;
        let model = job
            .requirements
            .model_id
            .clone()
            .or_else(|| {
                job.input
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .ok_or_else(|| {
                BackendError::Rejected("requirements.model_id or input.model required".into())
            })?;
        let max_tokens = job
            .input
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(512);

        let mut body = serde_json::json!({
            "model": model,
            "messages": [{ "role": "user", "content": prompt }],
            "stream": true,
            "stream_options": { "include_usage": true },
            "max_tokens": max_tokens,
        });
        // `input.think: false` disables hidden reasoning on thinking models (gemma4/qwen3 via Ollama).
        // Reasoning tokens are billed but never seen by the card, so single-step cards turn it off.
        // Ollama's OpenAI layer honors `think`; llama-server ignores unknown fields.
        if let Some(think) = job.input.get("think").and_then(|v| v.as_bool()) {
            body["think"] = serde_json::Value::Bool(think);
            if !think {
                body["reasoning_effort"] = serde_json::Value::String("none".into());
            }
        }

        let started = Instant::now();
        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?
            .error_for_status()
            .map_err(|e| BackendError::Execution(e.to_string()))?;

        let bytes = resp.bytes_stream();
        let stream = async_stream(bytes, started);
        Ok(Box::pin(stream))
    }
}

// ── Tool-calling completions for the coding-agent path (ADR-024) ───────────────────────────
//
// Everything below is new, additive surface used only by `crate::coder`'s agentic loop — no
// existing `Backend` trait method or caller is touched (see this file's own top doc and
// `crate::coder`'s module doc for why a coding session needs a fundamentally different call
// shape than the single-prompt `Backend::run` above: a running multi-role conversation plus a
// tool schema every turn, not one prompt string in and a text stream out). Ollama's
// OpenAI-compatible layer accepts a `tools` array on `/v1/chat/completions` for models trained
// for tool use (Hermes chief among them, per ADR-024) and returns `tool_calls` on the response
// message instead of (or, per some models, alongside) `content` when the model decides to call
// one.
//
// **Untested against a live model as of this writing.** This is written from Ollama's/OpenAI's
// published `tool_calls` response shape, not verified against a real Hermes/Ollama round-trip —
// see `crate::coder`'s module doc for the same caveat repeated where it matters operationally.
// Two specific risk areas worth flagging for whoever debugs the first real run: (1) some
// Ollama/llama.cpp versions omit `id` on each `tool_calls` entry (older OpenAI-compat shims did
// too) — handled here by treating it as optional (`#[serde(default)]`) and left to the caller
// (`crate::coder::LocalBrain`) to synthesize one if absent, since every downstream consumer
// needs a stable id to correlate a tool result back to its call; (2) a model can in principle
// emit a `tool_calls` array *and* non-empty `content` in the same message (some models narrate
// before calling a tool) — [`LlamaCppBackend::chat_with_tools`] treats any non-empty
// `tool_calls` as authoritative and ignores `content` in that case, since the agent loop's turn
// model ([`crate::coder::BrainTurn`]) has nowhere to put "text alongside a tool call".
//
// Deliberately **not streamed** (`"stream": false`), unlike `Backend::run` above: a tool call's
// arguments are a single JSON-encoded string that can't be acted on half-received, and streamed
// tool-call deltas are exactly the part of the OpenAI-compatible surface that varies most
// between server implementations/versions — a plain, complete response is the more portable
// choice for this early pass.

/// One message in an OpenAI-compatible tool-calling chat request/response, as spoken by Ollama's
/// `/v1/chat/completions`. This is `LlamaCppBackend`'s own wire shape — [`crate::coder`] defines
/// its own brain-agnostic `BrainMessage` and converts to/from this one (`crate::coder::LocalBrain`),
/// so this type has no reason to be used outside this module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallOut>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// One tool call the model asked for, in OpenAI's `tool_calls[i]` shape. `function.arguments` is
/// a JSON-encoded *string* (not a nested object) per that spec — the caller
/// (`crate::coder::LocalBrain`) parses it into a `serde_json::Value` for `crate::coder`'s own,
/// backend-agnostic vocabulary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallOut {
    /// Optional on the wire (see this section's doc, risk 1) — `crate::coder::LocalBrain`
    /// synthesizes a stable id when a server omits it.
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default = "tool_call_kind_function")]
    pub kind: String,
    pub function: ToolCallFunction,
}
fn tool_call_kind_function() -> String {
    "function".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    /// JSON-encoded arguments object, per the OpenAI function-calling wire format.
    #[serde(default)]
    pub arguments: String,
}

/// One tool's advertised schema, in OpenAI's function-calling `tools[i]` shape — the format
/// [`crate::coder::ToolSpec`] is expressed in and this method serializes verbatim.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSchema {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunctionSchema,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolFunctionSchema {
    pub name: String,
    pub description: String,
    /// A JSON Schema object describing the tool's arguments.
    pub parameters: serde_json::Value,
}

/// What one [`LlamaCppBackend::chat_with_tools`] call produced.
#[derive(Debug, Clone)]
pub enum ToolChatResult {
    /// No (or an empty) `tool_calls` in the response — a plain text reply.
    Text(String),
    /// A non-empty `tool_calls` array. Never constructed with an empty `Vec`.
    ToolCalls(Vec<ToolCallOut>),
}

#[derive(Deserialize)]
struct ToolChatResponse {
    #[serde(default)]
    choices: Vec<ToolChatChoice>,
    #[serde(default)]
    usage: Option<SseUsage>,
}
#[derive(Deserialize)]
struct ToolChatChoice {
    message: ToolChatResponseMessage,
}
#[derive(Deserialize)]
struct ToolChatResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallOut>>,
}

impl LlamaCppBackend {
    /// One non-streaming OpenAI-compatible tool-calling completion — `crate::coder`'s agentic
    /// loop calls this once per turn, handing it the whole conversation so far (system/user/
    /// assistant/tool messages) and the fixed four-tool schema.
    ///
    /// `tools` empty omits the `tools`/`tool_choice` fields from the request entirely, rather
    /// than sending `"tools": []`, since some OpenAI-compatible servers treat an
    /// empty-but-present `tools` array differently from an absent one; `crate::coder` always
    /// passes the full four-tool schema in practice; an empty slice is only ever a fallback.
    ///
    /// See this file's section doc above for what's tested vs. inferred from the API shape.
    pub async fn chat_with_tools(
        &self,
        model: &str,
        messages: &[ToolChatMessage],
        tools: &[ToolSchema],
        max_tokens: u64,
    ) -> Result<(ToolChatResult, Usage), BackendError> {
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": false,
            "max_tokens": max_tokens,
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)
                .map_err(|e| BackendError::Rejected(format!("bad tool schema: {e}")))?;
            body["tool_choice"] = serde_json::Value::String("auto".into());
        }
        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?
            .error_for_status()
            .map_err(|e| BackendError::Execution(e.to_string()))?;
        let parsed: ToolChatResponse = resp
            .json()
            .await
            .map_err(|e| BackendError::Execution(format!("bad tool-calling response: {e}")))?;
        let usage = parsed
            .usage
            .map(|u| Usage {
                tokens_in: u.prompt_tokens,
                tokens_out: u.completion_tokens,
                compute_seconds: 0.0,
            })
            .unwrap_or_default();
        let message = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| BackendError::Execution("tool-calling response had no choices".into()))?
            .message;
        match message.tool_calls {
            Some(calls) if !calls.is_empty() => Ok((ToolChatResult::ToolCalls(calls), usage)),
            _ => Ok((ToolChatResult::Text(message.content.unwrap_or_default()), usage)),
        }
    }
}

/// Parse an SSE byte stream into [`Chunk`]s. Kept as a free function so it can
/// be unit-tested against canned llama-server / Ollama transcripts.
fn async_stream(
    bytes: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    started: Instant,
) -> impl futures::Stream<Item = Result<Chunk, BackendError>> + Send {
    futures::stream::unfold(
        (
            Box::pin(bytes),
            Vec::<u8>::new(),
            0u64,
            None::<Usage>,
            false,
        ),
        move |(mut bytes, mut buf, mut deltas, mut usage, done)| async move {
            if done {
                return None;
            }
            loop {
                // Emit any complete SSE event already buffered.
                if let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                    let event = buf.drain(..pos + 2).collect::<Vec<u8>>();
                    let text = String::from_utf8_lossy(&event);
                    let data: String = text
                        .lines()
                        .filter_map(|l| l.strip_prefix("data:"))
                        .map(|s| s.trim())
                        .collect::<Vec<_>>()
                        .join("\n");
                    if data.is_empty() {
                        continue;
                    }
                    if data == "[DONE]" {
                        let u = usage.unwrap_or_else(|| {
                            tracing::warn!("server sent no usage; metering from delta count");
                            Usage {
                                tokens_in: 0,
                                tokens_out: deltas,
                                compute_seconds: 0.0,
                            }
                        });
                        let u = Usage {
                            compute_seconds: started.elapsed().as_secs_f64(),
                            ..u
                        };
                        return Some((Ok(Chunk::done(u)), (bytes, buf, deltas, usage, true)));
                    }
                    match serde_json::from_str::<SseChunk>(&data) {
                        Ok(c) => {
                            if let Some(u) = c.usage {
                                usage = Some(Usage {
                                    tokens_in: u.prompt_tokens,
                                    tokens_out: u.completion_tokens,
                                    compute_seconds: 0.0,
                                });
                            }
                            if let Some(choice) = c.choices.first() {
                                if let Some(content) = &choice.delta.content {
                                    if !content.is_empty() {
                                        deltas += 1;
                                        return Some((
                                            Ok(Chunk::text(content.clone())),
                                            (bytes, buf, deltas, usage, false),
                                        ));
                                    }
                                }
                                let _ = choice.finish_reason.as_deref();
                            }
                            continue;
                        }
                        Err(e) => {
                            return Some((
                                Err(BackendError::Execution(format!(
                                    "bad SSE json: {e}: {data}"
                                ))),
                                (bytes, buf, deltas, usage, true),
                            ))
                        }
                    }
                }
                // Need more bytes.
                match bytes.next().await {
                    Some(Ok(b)) => buf.extend_from_slice(&b),
                    Some(Err(e)) => {
                        return Some((
                            Err(BackendError::Execution(e.to_string())),
                            (bytes, buf, deltas, usage, true),
                        ))
                    }
                    None => {
                        // Stream ended without [DONE]; finish with what we have.
                        let u = usage.unwrap_or(Usage {
                            tokens_in: 0,
                            tokens_out: deltas,
                            compute_seconds: 0.0,
                        });
                        let u = Usage {
                            compute_seconds: started.elapsed().as_secs_f64(),
                            ..u
                        };
                        return Some((Ok(Chunk::done(u)), (bytes, buf, deltas, usage, true)));
                    }
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::collect;

    #[tokio::test]
    async fn parses_llama_server_style_sse_with_usage() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );
        // Split across awkward byte boundaries to exercise buffering.
        let parts: Vec<Result<bytes::Bytes, reqwest::Error>> = sse
            .as_bytes()
            .chunks(13)
            .map(|c| Ok(bytes::Bytes::copy_from_slice(c)))
            .collect();
        let stream = async_stream(futures::stream::iter(parts), Instant::now());
        let (text, usage) = collect(Box::pin(stream)).await.unwrap();
        assert_eq!(text, "Hello");
        assert_eq!((usage.tokens_in, usage.tokens_out), (7, 2));
    }

    #[tokio::test]
    async fn falls_back_to_delta_count_without_usage() {
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\ndata: [DONE]\n\n";
        let stream = async_stream(
            futures::stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(sse))]),
            Instant::now(),
        );
        let (text, usage) = collect(Box::pin(stream)).await.unwrap();
        assert_eq!(text, "ab");
        assert_eq!(usage.tokens_out, 2);
    }
}
