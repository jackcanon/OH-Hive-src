//! whisper.cpp adapter (ADR-003 D61/M8) — the Hive-distributed Speech backend.
//!
//! Talks to `whisper.cpp`'s `examples/server` (the `whisper-server` binary),
//! which exposes a plain `POST /inference` multipart endpoint: send an audio
//! file, get back `{"text": "..."}`. Unlike llama-server's chat-completions
//! protocol this is not streaming and reports no token usage — whisper
//! doesn't have "tokens" in the billed sense, so metering falls back to wall-
//! clock `compute_seconds` (ADR-002's open item: compute-seconds × hardware
//! class for non-token modalities). Process management (spawning
//! `whisper-server` with a ggml model) is the node app's job, same split as
//! `llama_cpp.rs` — this adapter only needs a base URL.
//!
//! This is the *network* speech-to-text path: any node running this backend
//! can be scheduled another member's paid transcription card. It is separate
//! from, and does not replace, the Mac app's on-device Apple Speech
//! transcription (`apps/desktop-swift`) — that one is local-only per the
//! ADR-018 guardrail (decision 11) and never serves Hive-distributed work.
//! Running both is the point: whisper.cpp works on every OS this project
//! supports (Windows/Linux/Intel Mac included) and is what actually earns
//! $honey; Apple's on-device transcriber is a free, private, Apple Silicon-
//! only convenience for the node owner's own files.

use super::{Backend, BackendError, Chunk, ChunkStream};
use crate::capability::{Capabilities, GpuVendor, Hardware, Modality, ModelRef, ToolsLevel};
use crate::job::Job;
use crate::ledger::Usage;
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Instant;

pub struct WhisperCppBackend {
    /// e.g. `http://127.0.0.1:8081` (whisper.cpp's `whisper-server`).
    pub base_url: String,
    /// Model id this instance was started with, e.g. "ggml-large-v3-turbo".
    /// whisper-server serves one model per process (unlike llama-server/Ollama's
    /// multi-model `/v1/models`), so this is advertised rather than discovered.
    pub model_id: String,
    client: reqwest::Client,
}

impl WhisperCppBackend {
    pub fn new(base_url: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model_id: model_id.into(),
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Deserialize)]
struct InferenceResponse {
    #[serde(default)]
    text: String,
}

#[async_trait]
impl Backend for WhisperCppBackend {
    fn name(&self) -> &'static str {
        "whisper"
    }

    async fn capabilities(&self) -> Result<Capabilities, BackendError> {
        Ok(Capabilities {
            // Hardware probe is the node app's job (ADR-010 step 2); placeholder here,
            // same convention as llama_cpp.rs.
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
            modalities: vec![Modality::Speech],
            models: vec![ModelRef {
                id: self.model_id.clone(),
                modality: Modality::Speech,
                backend: "whisper".into(),
            }],
            allow_internet: false,
            tools_level: ToolsLevel::SandboxedTools,
            storage_gb_offered: None,
            shard_capable: None,
        })
    }

    async fn run<'a>(&'a self, job: &'a Job) -> Result<ChunkStream<'a>, BackendError> {
        // The audio bytes travel as a staged local file, not inline JSON (ADR-006's
        // artifact_get tool already stages fetched artifacts at a known path — see
        // `tools::run_artifact_get` — so a Speech job's input just names that path).
        let audio_path = job
            .input
            .get("audio_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BackendError::Rejected("input.audio_path missing".into()))?
            .to_string();
        let language = job
            .input
            .get("language")
            .and_then(|v| v.as_str())
            .map(String::from);

        let bytes = tokio::fs::read(&audio_path)
            .await
            .map_err(|e| BackendError::Rejected(format!("can't read {audio_path}: {e}")))?;
        let filename = std::path::Path::new(&audio_path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("audio.wav")
            .to_string();

        let mut form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(bytes).file_name(filename),
            )
            .text("response_format", "json");
        if let Some(lang) = language {
            form = form.text("language", lang);
        }

        let started = Instant::now();
        let resp = self
            .client
            .post(format!("{}/inference", self.base_url))
            .multipart(form)
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?
            .error_for_status()
            .map_err(|e| BackendError::Execution(e.to_string()))?
            .json::<InferenceResponse>()
            .await
            .map_err(|e| BackendError::Execution(format!("bad whisper-server response: {e}")))?;

        let usage = Usage {
            tokens_in: 0,
            tokens_out: 0,
            compute_seconds: started.elapsed().as_secs_f64(),
        };
        let text = resp.text.trim().to_string();
        let stream = futures::stream::iter(vec![Ok(Chunk::text(text)), Ok(Chunk::done(usage))]);
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::collect;

    #[test]
    fn parses_inference_json_response() {
        let raw = r#"{"text":" hello from whisper "}"#;
        let parsed: InferenceResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.text.trim(), "hello from whisper");
    }

    #[test]
    fn missing_text_field_defaults_empty() {
        let raw = r#"{}"#;
        let parsed: InferenceResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.text, "");
    }

    #[tokio::test]
    async fn stream_yields_text_then_done_with_compute_seconds() {
        let usage = Usage {
            tokens_in: 0,
            tokens_out: 0,
            compute_seconds: 0.25,
        };
        let stream = futures::stream::iter(vec![
            Ok(Chunk::text("a transcript".to_string())),
            Ok(Chunk::done(usage)),
        ]);
        let (text, usage) = collect(Box::pin(stream)).await.unwrap();
        assert_eq!(text, "a transcript");
        assert_eq!(usage.tokens_out, 0);
        assert!(usage.compute_seconds > 0.0);
    }
}
