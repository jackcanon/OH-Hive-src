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
    strict_completion: bool,
}

impl LlamaCppBackend {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            strict_completion: false,
        }
    }

    /// Private Bots calls must stay on this machine, including redirects and proxy handling.
    pub fn local_only(base_url: &str) -> Result<Self, BackendError> {
        let url = reqwest::Url::parse(base_url)
            .map_err(|_| BackendError::Rejected("Invalid local model URL".into()))?;
        let loopback = url
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| ip.is_loopback());
        if !loopback
            || !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(BackendError::Rejected(
                "Use a loopback model origin such as http://127.0.0.1:11434".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| BackendError::Unavailable("Cannot create local model client".into()))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').into(),
            client,
            strict_completion: true,
        })
    }

    /// Metadata only: never loads a model or starts inference. None means this server
    /// does not report tool support (e.g. llama-server), not confirmed compatibility.
    /// What the server said, not just the status line.
    ///
    /// `error_for_status()` throws the response body away, and for this backend the body is the
    /// entire answer. Ollama refuses a tool-calling request for a model that cannot do tool
    /// calling with `{"error":{"message":"<model> does not support tools"}}` -- exactly the
    /// sentence a caller needs -- and reqwest reduces that to "HTTP status client error (400 Bad
    /// Request)". A code card that picked such a model therefore failed in under two seconds
    /// with a message that named neither the model nor the reason, and looked for all the world
    /// like the node's Ollama was broken.
    async fn status_error(response: reqwest::Response) -> String {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        // Ollama and OpenAI both nest the human sentence at error.message; anything else is
        // shown raw rather than dropped, because a body we do not recognise still says more
        // than a status code does.
        let detail = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message").or(Some(e)))
                    .map(|m| {
                        m.as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| m.to_string())
                    })
            })
            .unwrap_or_else(|| body.trim().chars().take(400).collect());
        if detail.is_empty() {
            format!("{status}")
        } else {
            format!("{status}: {detail}")
        }
    }

    pub async fn model_tool_support(&self, model: &str) -> Result<Option<bool>, BackendError> {
        let response = self
            .client
            .post(format!("{}/api/show", self.base_url))
            .timeout(std::time::Duration::from_secs(3))
            .json(&serde_json::json!({"model": model}))
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        if matches!(response.status().as_u16(), 404 | 405) {
            return Ok(None);
        }
        if let Err(status) = response.error_for_status_ref() {
            let _ = status;
            return Err(BackendError::Unavailable(
                Self::status_error(response).await,
            ));
        }
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        Ok(value
            .get("capabilities")
            .and_then(serde_json::Value::as_array)
            .map(|caps| caps.iter().any(|cap| cap.as_str() == Some("tools"))))
    }

    async fn list_models(&self) -> Result<Vec<ModelRef>, BackendError> {
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
        // Ollama's OpenAI-compatible /v1/models omits weight bytes; /api/tags supplies
        // them. llama-server may not implement it, so this enrichment is best-effort.
        let sizes = match self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                response.json::<OllamaTags>().await.ok()
            }
            _ => None,
        };
        Ok(r.data
            .into_iter()
            .map(|m| ModelRef {
                size_bytes: sizes.as_ref().and_then(|tags| tags.size_for(&m.id)),
                id: m.id,
                modality: Modality::Text,
                backend: "llama_cpp".into(),
            })
            .collect())
    }
}

#[derive(Deserialize)]
struct OllamaTags {
    models: Vec<OllamaModelSize>,
}
#[derive(Deserialize)]
struct OllamaModelSize {
    name: String,
    #[serde(default)]
    model: Option<String>,
    size: u64,
}
impl OllamaTags {
    fn size_for(&self, id: &str) -> Option<u64> {
        self.models
            .iter()
            .find(|m| m.name == id || m.model.as_deref() == Some(id))
            .map(|m| m.size)
            .filter(|size| *size > 0)
    }
}

#[cfg(test)]
mod model_size_tests {
    use super::*;
    #[tokio::test]
    async fn tool_metadata_does_not_infer_or_assume_compatibility() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (status, body, expected) in [
            (
                "200 OK",
                r#"{"capabilities":["completion","tools"]}"#,
                Some(true),
            ),
            (
                "200 OK",
                r#"{"capabilities":["completion","vision"]}"#,
                Some(false),
            ),
            ("200 OK", r#"{}"#, None),
            ("404 Not Found", r#"{}"#, None),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 4096];
                let n = socket.read(&mut bytes).await.unwrap();
                assert!(String::from_utf8_lossy(&bytes[..n]).starts_with("POST /api/show "));
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            });
            assert_eq!(
                LlamaCppBackend::local_only(&format!("http://{addr}"))
                    .unwrap()
                    .model_tool_support("test-model")
                    .await
                    .unwrap(),
                expected
            );
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn capabilities_enriches_openai_listing_with_ollama_weight_bytes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for (path, body) in [
                ("/v1/models", r#"{"data":[{"id":"qwen:27b"}]}"#),
                (
                    "/api/tags",
                    r#"{"models":[{"name":"qwen:27b","size":16000000000}]}"#,
                ),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                let count = stream.read(&mut request).await.unwrap();
                assert!(
                    String::from_utf8_lossy(&request[..count]).starts_with(&format!("GET {path} "))
                );
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let caps = LlamaCppBackend::local_only(&format!("http://{address}"))
            .unwrap()
            .capabilities()
            .await
            .unwrap();
        assert_eq!(caps.models[0].size_bytes, Some(16_000_000_000));
        server.await.unwrap();
    }

    #[test]
    fn tags_match_exact_model_and_ignore_zero_or_unrelated_sizes() {
        let tags: OllamaTags = serde_json::from_str(r#"{"models":[{"name":"qwen:27b","model":"qwen:27b","size":16000000000},{"name":"empty","size":0}]}"#).unwrap();
        assert_eq!(tags.size_for("qwen:27b"), Some(16_000_000_000));
        assert_eq!(tags.size_for("qwen:8b"), None);
        assert_eq!(tags.size_for("empty"), None);
    }
}

#[derive(Deserialize)]
struct SseChunk {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    id: Option<String>,
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
            models,
            allow_internet: false,
            tools_level: ToolsLevel::SandboxedTools,
            storage_gb_offered: None,
            shard_capable: None,
            acceptance: crate::capability::Capabilities::RUNS_ACCEPTANCE,
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
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        if resp.error_for_status_ref().is_err() {
            return Err(BackendError::Execution(Self::status_error(resp).await));
        }

        let bytes = resp.bytes_stream();
        let stream = async_stream_policy_observed(
            bytes,
            started,
            self.strict_completion,
            Some(ModelObservation::new(job.id.to_string(), model)),
        );
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
    model: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    choices: Vec<ToolChatChoice>,
    #[serde(default)]
    usage: Option<SseUsage>,
}
#[derive(Deserialize)]
struct ToolChatChoice {
    message: ToolChatResponseMessage,
    /// `"stop"`, `"tool_calls"`, `"length"`, … — read solely so a turn the server cut off at
    /// `max_tokens` can be refused instead of executed. See `chat_with_tools`.
    #[serde(default)]
    finish_reason: Option<String>,
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
            .map_err(|e| BackendError::Unavailable(e.to_string()))?;
        if resp.error_for_status_ref().is_err() {
            return Err(BackendError::Execution(Self::status_error(resp).await));
        }
        let parsed: ToolChatResponse = resp
            .json()
            .await
            .map_err(|e| BackendError::Execution(format!("bad tool-calling response: {e}")))?;
        ModelObservation::new(uuid::Uuid::new_v4().to_string(), model.to_owned())
            .record(parsed.model.as_deref(), parsed.id.as_deref());
        let usage = parsed
            .usage
            .map(|u| Usage {
                tokens_in: u.prompt_tokens,
                tokens_out: u.completion_tokens,
                compute_seconds: 0.0,
            })
            .unwrap_or_default();
        let choice = parsed.choices.into_iter().next().ok_or_else(|| {
            BackendError::Execution("tool-calling response had no choices".into())
        })?;
        // A turn the server cut off at `max_tokens` must not be executed. Whatever it contains is
        // a *prefix*: a tool call missing its closing brace (which `crate::coder`'s parse then
        // reads as "no arguments at all"), or a final answer that stops mid-sentence. Failing the
        // turn here surfaces the one thing that actually fixes it — a bigger budget — instead of
        // letting a half-formed call reach `execute_tool`.
        if choice.finish_reason.as_deref() == Some("length") {
            return Err(BackendError::Execution(format!(
                "completion cut off at the {max_tokens}-token limit (finish_reason=length); \
                 raise the card's required_capabilities.max_tokens"
            )));
        }
        let message = choice.message;
        match message.tool_calls {
            Some(calls) if !calls.is_empty() => Ok((ToolChatResult::ToolCalls(calls), usage)),
            _ => Ok((
                ToolChatResult::Text(message.content.unwrap_or_default()),
                usage,
            )),
        }
    }
}

/// Parse an SSE byte stream into [`Chunk`]s. Kept as a free function so it can
/// be unit-tested against canned llama-server / Ollama transcripts.
// Metadata only: never log prompts, generated text, URLs or credentials.
// A different string may be an alias, not necessarily different weights.
struct ModelObservation {
    request_id: String,
    requested: String,
    seen: Option<String>,
}
impl ModelObservation {
    fn new(request_id: String, requested: String) -> Self {
        Self {
            request_id,
            requested,
            seen: None,
        }
    }
    fn record(&mut self, reported: Option<&str>, completion_id: Option<&str>) {
        let Some(reported) = reported.filter(|s| !s.is_empty()) else {
            return;
        };
        // Bound remote metadata before comparing/logging, including repeated SSE frames.
        let reported: String = reported
            .chars()
            .filter(|c| !c.is_control())
            .take(256)
            .collect();
        if self.seen.as_deref() == Some(reported.as_str()) {
            return;
        }
        let completion: String = completion_id.unwrap_or("").chars().take(128).collect();
        tracing::info!(request_id = %self.request_id, requested_model = %self.requested,
            reported_model = %reported, completion_id = %completion,
            model_name_matches = (self.requested == reported), "local model response identity");
        self.seen = Some(reported);
    }
}

#[cfg(test)]
fn async_stream_policy(
    bytes: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    started: Instant,
    strict: bool,
) -> impl futures::Stream<Item = Result<Chunk, BackendError>> + Send {
    async_stream_policy_observed(bytes, started, strict, None)
}

fn async_stream_policy_observed(
    bytes: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    started: Instant,
    strict: bool,
    observation: Option<ModelObservation>,
) -> impl futures::Stream<Item = Result<Chunk, BackendError>> + Send {
    futures::stream::unfold(
        (
            Box::pin(bytes),
            Vec::<u8>::new(),
            0u64,
            None::<Usage>,
            false,
            // `finish_reason == "length"` seen on an earlier event: OpenAI-compatible servers send
            // it on the last content-bearing chunk, *before* `data: [DONE]`, so it has to be
            // carried across iterations to reach whichever exit emits the `done` chunk.
            false,
            observation,
        ),
        move |(mut bytes, mut buf, mut deltas, mut usage, done, mut truncated, mut observation)| async move {
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
                        let done_chunk = if truncated {
                            Chunk::done_truncated(u)
                        } else {
                            Chunk::done(u)
                        };
                        return Some((
                            Ok(done_chunk),
                            (bytes, buf, deltas, usage, true, truncated, observation),
                        ));
                    }
                    match serde_json::from_str::<SseChunk>(&data) {
                        Ok(c) => {
                            if let Some(ref mut obs) = observation {
                                obs.record(c.model.as_deref(), c.id.as_deref());
                            }
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
                                            (
                                                bytes,
                                                buf,
                                                deltas,
                                                usage,
                                                false,
                                                truncated,
                                                observation,
                                            ),
                                        ));
                                    }
                                }
                                // Recorded, not acted on here: the caller needs the
                                // text already streamed *and* the fact that it stops
                                // short, so this rides along to the `done` chunk.
                                if choice.finish_reason.as_deref() == Some("length") {
                                    truncated = true;
                                }
                            }
                            continue;
                        }
                        Err(e) => {
                            return Some((
                                Err(BackendError::Execution(format!(
                                    "bad SSE json: {e}: {data}"
                                ))),
                                (bytes, buf, deltas, usage, true, truncated, observation),
                            ))
                        }
                    }
                }
                // Need more bytes.
                match bytes.next().await {
                    Some(Ok(b)) => {
                        if strict && buf.len().saturating_add(b.len()) > 256 * 1024 {
                            return Some((
                                Err(BackendError::Execution(
                                    "Local SSE frame exceeds limit".into(),
                                )),
                                (bytes, buf, deltas, usage, true, truncated, observation),
                            ));
                        }
                        buf.extend_from_slice(&b)
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(BackendError::Execution(e.to_string())),
                            (bytes, buf, deltas, usage, true, truncated, observation),
                        ))
                    }
                    None => {
                        if strict {
                            return Some((
                                Err(BackendError::Execution(
                                    "Local stream ended before DONE".into(),
                                )),
                                (bytes, buf, deltas, usage, true, truncated, observation),
                            ));
                        }
                        // Legacy card behavior: finish with what we have.
                        let u = usage.unwrap_or(Usage {
                            tokens_in: 0,
                            tokens_out: deltas,
                            compute_seconds: 0.0,
                        });
                        let u = Usage {
                            compute_seconds: started.elapsed().as_secs_f64(),
                            ..u
                        };
                        let done_chunk = if truncated {
                            Chunk::done_truncated(u)
                        } else {
                            Chunk::done(u)
                        };
                        return Some((
                            Ok(done_chunk),
                            (bytes, buf, deltas, usage, true, truncated, observation),
                        ));
                    }
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn response_identity_retains_reported_model_without_rewriting_request() {
        let chunk: super::SseChunk =
            serde_json::from_str(r#"{"id":"completion-1","model":"served:tag","choices":[]}"#)
                .unwrap();
        let mut observation =
            super::ModelObservation::new("request-1".into(), "requested:tag".into());
        observation.record(chunk.model.as_deref(), chunk.id.as_deref());
        assert_eq!(observation.requested, "requested:tag");
        assert_eq!(observation.seen.as_deref(), Some("served:tag"));
        observation.record(None, None);
        assert_eq!(observation.seen.as_deref(), Some("served:tag"));
        let tool: super::ToolChatResponse =
            serde_json::from_str(r#"{"model":"tool:tag","id":"tool-1","choices":[]}"#).unwrap();
        assert_eq!(tool.model.as_deref(), Some("tool:tag"));
        let omitted: super::SseChunk = serde_json::from_str(r#"{"choices":[]}"#).unwrap();
        assert!(omitted.model.is_none());
    }

    use super::*;
    use crate::backend::collect;

    // A code card that picked a model without tool support failed in under two seconds with
    // "HTTP status client error (400 Bad Request)" -- naming neither the model nor the reason,
    // and reading like the node's Ollama was down. Ollama had in fact said exactly what was
    // wrong; `error_for_status()` threw the sentence away. Observed on Overgaard, 2026-09-18.
    #[tokio::test]
    async fn a_refusal_carries_what_the_server_said_not_just_the_status() {
        use axum::{routing::post, Json, Router};
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": {
                        "message": "registry.ollama.ai/library/gemma3:4b does not support tools",
                        "type": "invalid_request_error"
                    }})),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let backend = LlamaCppBackend::new(&base);
        let error = backend
            .chat_with_tools(
                "gemma3:4b",
                &[ToolChatMessage {
                    role: "user".into(),
                    content: Some("hello".into()),
                    tool_calls: None,
                    tool_call_id: None,
                }],
                &[],
                16,
            )
            .await
            .expect_err("a 400 must not be reported as success")
            .to_string();

        assert!(
            error.contains("does not support tools") && error.contains("gemma3:4b"),
            "the refusal must carry the server's own sentence: {error}"
        );
    }

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
        let stream = async_stream_policy(futures::stream::iter(parts), Instant::now(), false);
        let completion = collect(Box::pin(stream)).await.unwrap();
        let (text, usage) = (completion.text, completion.usage);
        assert_eq!(text, "Hello");
        assert_eq!((usage.tokens_in, usage.tokens_out), (7, 2));
    }

    #[tokio::test]
    async fn falls_back_to_delta_count_without_usage() {
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\ndata: [DONE]\n\n";
        let stream = async_stream_policy(
            futures::stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(sse))]),
            Instant::now(),
            false,
        );
        let completion = collect(Box::pin(stream)).await.unwrap();
        let (text, usage) = (completion.text, completion.usage);
        assert_eq!(text, "ab");
        assert_eq!(usage.tokens_out, 2);
    }

    /// `finish_reason: "length"` arrives on an event *before* `[DONE]`, so the flag has to survive
    /// the fold iterations in between and still land on the `done` chunk. Without that, a
    /// completion cut off at the cap is indistinguishable from a finished one, and
    /// `crate::worker` ships half an artifact.
    #[tokio::test]
    async fn length_finish_reason_marks_the_completion_truncated() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"fn main() {\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":4}}\n\n",
            "data: [DONE]\n\n",
        );
        let stream = async_stream_policy(
            futures::stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(sse))]),
            Instant::now(),
            false,
        );
        let completion = collect(Box::pin(stream)).await.unwrap();
        assert_eq!(completion.text, "fn main() {");
        assert!(
            completion.truncated,
            "a length-capped completion must report itself truncated"
        );
    }

    /// The other half of the same guarantee: an ordinary `stop` must *not* be reported as
    /// truncated, or every card would fail.
    #[tokio::test]
    async fn stop_finish_reason_is_not_truncated() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"done\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":1}}\n\n",
            "data: [DONE]\n\n",
        );
        let stream = async_stream_policy(
            futures::stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(sse))]),
            Instant::now(),
            false,
        );
        let completion = collect(Box::pin(stream)).await.unwrap();
        assert_eq!(completion.text, "done");
        assert!(!completion.truncated);
    }

    /// A server that ends the stream without `[DONE]` still has to carry the flag through the
    /// non-strict "finish with what we have" exit — that is the path a card actually takes when a
    /// local llama-server drops the connection right after hitting the cap.
    #[tokio::test]
    async fn truncation_survives_a_stream_that_ends_without_done() {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"half\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
        );
        let stream = async_stream_policy(
            futures::stream::iter(vec![Ok::<_, reqwest::Error>(bytes::Bytes::from(sse))]),
            Instant::now(),
            false,
        );
        let completion = collect(Box::pin(stream)).await.unwrap();
        assert_eq!(completion.text, "half");
        assert!(completion.truncated);
    }
}
