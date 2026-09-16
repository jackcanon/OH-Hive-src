//! FFI wrapper for the M8 media backends (`hive_core::backend::whisper`/`comfyui`) -- lets the
//! Swift app drive them directly, the same way `hive run --backend whisper|comfyui` does from the
//! CLI, rather than going through the node's Hive-distributed worker loop (`worker_start` in
//! `lib.rs` only ever runs one backend, `LlamaCppBackend`, against the network's job queue --
//! extending the worker to dispatch across modalities is separate, bigger work, tracked
//! alongside the rest of M8's scheduler-side gaps).
//!
//! This is deliberately the same shape as `ChatEngine.swift`'s on-device Foundation Models call
//! and the new Transcribe tab's Apple `SpeechAnalyzer` path: "this app can do the thing right
//! now, using whatever's configured/available," in service of Hive being a daily-driver AI app
//! rather than only a compute-contribution tool. It is NOT Hive-network job scheduling -- these
//! calls go straight from this Mac to whatever whisper.cpp/ComfyUI instance `HIVE_WHISPER_URL`/
//! `HIVE_COMFYUI_URL` point at (this machine's own, if the owner is running one; nothing routes
//! the request to another member's node).

use crate::{HiveError, HiveNode, RUNTIME};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use hive_core::backend::comfyui::ComfyUiBackend;
use hive_core::backend::whisper::WhisperCppBackend;
use hive_core::backend::{collect, Backend};
use hive_core::capability::Requirements;
use hive_core::hub::HubClient;
use hive_core::job::{Job, JobKind};
use hive_core::nodeconfig;
use std::sync::Arc;

#[derive(uniffi::Record, Clone)]
pub struct TranscribeResult {
    pub text: String,
    pub compute_seconds: f64,
}

#[derive(uniffi::Record, Clone)]
pub struct GeneratedImage {
    /// Local file path (a temp PNG) -- the Swift side loads it with
    /// `NSImage(contentsOfFile:)`. Not yet uploaded to the artifact store (M9) since this call
    /// doesn't go through a card/project; see `comfyui.rs`'s own TODO on the same point.
    pub file_path: String,
    pub compute_seconds: f64,
}

fn ad_hoc_job(input: serde_json::Value) -> Job {
    Job {
        id: uuid::Uuid::new_v4(),
        kind: JobKind::Inference,
        project_id: uuid::Uuid::nil(),
        card_id: None,
        parent: None,
        requirements: Requirements::default(),
        input,
        resume_from: None,
        created_at: chrono::Utc::now(),
    }
}

#[uniffi::export]
impl HiveNode {
    /// Transcribe a local audio file via the Hive network's whisper.cpp path (as opposed to the
    /// Mac's own on-device Apple transcriber, which is a separate, non-FFI Swift-only feature --
    /// see `TranscribeEngine.swift`). Requires `HIVE_WHISPER_URL` to be configured (Settings ->
    /// Media backends) and pointed at a running whisper.cpp `whisper-server`.
    pub async fn transcribe_whisper(
        self: Arc<Self>,
        audio_path: String,
        language: Option<String>,
    ) -> Result<TranscribeResult, HiveError> {
        self.log("info", "transcribing via Hive network (whisper.cpp)")
            .await;
        let this = self.clone();
        let r = RUNTIME
            .spawn(async move {
                nodeconfig::export_env();
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let url = cfg.whisper_url.ok_or_else(|| {
                    HiveError::Failed(
                        "no whisper backend configured -- set it in Settings > Media backends"
                            .into(),
                    )
                })?;
                let model = cfg.whisper_model.unwrap_or_else(|| "unknown".into());
                let be = WhisperCppBackend::new(url, model);
                let mut input = serde_json::json!({ "audio_path": audio_path });
                if let Some(lang) = language {
                    input["language"] = serde_json::Value::String(lang);
                }
                let job = ad_hoc_job(input);
                let stream = be
                    .run(&job)
                    .await
                    .map_err(|e| HiveError::Failed(e.to_string()))?;
                let hive_core::backend::Completion { text, usage, truncated } = collect(stream)
                    .await
                    .map_err(|e| HiveError::Failed(e.to_string()))?;
                if truncated {
                    return Err(HiveError::Failed("Media result was incomplete. Please retry.".into()));
                }
                Ok(TranscribeResult {
                    text,
                    compute_seconds: usage.compute_seconds,
                })
            })
            .await
            .map_err(|e| HiveError::Failed(format!("transcribe_whisper task panicked: {e}")))?;
        match &r {
            Ok(res) => {
                this.log("ok", format!("transcribed in {:.1}s", res.compute_seconds))
                    .await
            }
            Err(e) => {
                this.log("error", format!("transcription failed: {e}"))
                    .await
            }
        }
        r
    }

    /// Generate an image via the Hive network's ComfyUI path. Requires `HIVE_COMFYUI_URL` and
    /// `HIVE_COMFYUI_CHECKPOINT` to be configured (Settings -> Media backends) and pointed at a
    /// running, API-mode ComfyUI instance with that checkpoint installed. Can take anywhere from
    /// several seconds to a few minutes depending on the model/hardware -- callers should show
    /// progress, not a blocking spinner with no explanation.
    pub async fn generate_image_comfyui(
        self: Arc<Self>,
        prompt: String,
        negative_prompt: Option<String>,
    ) -> Result<GeneratedImage, HiveError> {
        self.log(
            "info",
            format!("generating image via Hive network (ComfyUI): \u{201c}{prompt}\u{201d}"),
        )
        .await;
        let this = self.clone();
        let r = RUNTIME
            .spawn(async move {
                nodeconfig::export_env();
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let url = cfg.comfyui_url.ok_or_else(|| {
                    HiveError::Failed(
                        "no ComfyUI backend configured -- set it in Settings > Media backends"
                            .into(),
                    )
                })?;
                let checkpoint = cfg.comfyui_checkpoint.ok_or_else(|| {
                    HiveError::Failed(
                        "no ComfyUI checkpoint configured -- set it in Settings > Media backends"
                            .into(),
                    )
                })?;
                let be = ComfyUiBackend::new(url, checkpoint);
                let input = serde_json::json!({
                    "prompt": prompt,
                    "negative_prompt": negative_prompt.unwrap_or_default(),
                });
                let job = ad_hoc_job(input);
                let stream = be
                    .run(&job)
                    .await
                    .map_err(|e| HiveError::Failed(e.to_string()))?;
                let hive_core::backend::Completion { text: file_path, usage, truncated } = collect(stream)
                    .await
                    .map_err(|e| HiveError::Failed(e.to_string()))?;
                if truncated {
                    return Err(HiveError::Failed("Media result was incomplete. Please retry.".into()));
                }
                Ok(GeneratedImage {
                    file_path,
                    compute_seconds: usage.compute_seconds,
                })
            })
            .await
            .map_err(|e| HiveError::Failed(format!("generate_image_comfyui task panicked: {e}")))?;
        match &r {
            Ok(res) => {
                this.log(
                    "ok",
                    format!("image generated in {:.1}s", res.compute_seconds),
                )
                .await
            }
            Err(e) => {
                this.log("error", format!("image generation failed: {e}"))
                    .await
            }
        }
        r
    }

    /// Generate an image via the hub's hosted path (OpenAI, `generate-image` Edge Function) --
    /// the default in the Swift app's Generate tab (tasks #125-128). BYOK-only (2026-09-13): the
    /// member's own OpenAI key, added in Settings > Providers, is what actually gets billed -- no
    /// Honey involved either way. Unlike `generate_image_comfyui`, this one goes through the hub
    /// (node-key authenticated, same as `submit_feature_request`), because the actual OpenAI call
    /// happens server-side where the member's key is decrypted from Vault, never on this Mac.
    pub async fn generate_image_hosted(
        self: Arc<Self>,
        prompt: String,
        negative_prompt: Option<String>,
    ) -> Result<GeneratedImage, HiveError> {
        self.log(
            "info",
            format!("generating image via Hive's hosted path (OpenAI): \u{201c}{prompt}\u{201d}"),
        )
        .await;
        let this = self.clone();
        let r = RUNTIME
            .spawn(async move {
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let key = cfg
                    .node_key
                    .clone()
                    .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
                let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
                let started = std::time::Instant::now();
                let result = hub
                    .generate_image_hosted(&prompt, negative_prompt.as_deref())
                    .await
                    .map_err(|e| {
                        // The Edge Function's JSON body rides along inside HubError::Rejected's
                        // message (see HubClient::edge_function) -- give the common no-key case a
                        // plain-English message instead of surfacing the raw {"error":"no_byo_key",...}
                        // blob to the person using the app.
                        let msg = e.to_string();
                        if msg.contains("no_byo_key") {
                            HiveError::Failed(
                                "add your OpenAI key in Settings > Providers to generate images this way, or switch to Local (ComfyUI)".into(),
                            )
                        } else {
                            HiveError::from(e)
                        }
                    })?;
                let bytes = STANDARD.decode(&result.image_base64).map_err(|e| {
                    HiveError::Failed(format!("hub returned an unreadable image: {e}"))
                })?;
                let tmp = std::env::temp_dir().join(format!("hive-hosted-{}.png", uuid::Uuid::new_v4()));
                tokio::fs::write(&tmp, &bytes)
                    .await
                    .map_err(|e| HiveError::Failed(format!("couldn't save the image: {e}")))?;
                Ok(GeneratedImage {
                    file_path: tmp.display().to_string(),
                    compute_seconds: started.elapsed().as_secs_f64(),
                })
            })
            .await
            .map_err(|e| HiveError::Failed(format!("generate_image_hosted task panicked: {e}")))?;
        match &r {
            Ok(res) => {
                this.log(
                    "ok",
                    format!("image generated in {:.1}s", res.compute_seconds),
                )
                .await
            }
            Err(e) => {
                this.log("error", format!("hosted image generation failed: {e}"))
                    .await
            }
        }
        r
    }
}
