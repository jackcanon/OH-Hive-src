//! ComfyUI adapter (ADR-003 D61/D62, M8) — the Image modality's first slice.
//!
//! Talks to a running ComfyUI instance's REST API (`--listen`, API mode):
//! `POST /prompt` queues a workflow graph, `GET /history/{id}` is polled until
//! it reports outputs, `GET /view` fetches the resulting image bytes. Process
//! management — starting ComfyUI itself inside the node app's bundled `uv`
//! venv (ADR-003 D62) — is a separate, not-yet-built concern, same split
//! `llama_cpp.rs` and `whisper.rs` make: this adapter only needs a base URL
//! for an instance that's already running.
//!
//! Scope of this first slice: text-to-image only, one checkpoint per backend
//! instance (mirrors `whisper.rs`'s one-model-per-process shape — no live
//! `/object_info` model discovery yet, since that introspection endpoint's
//! shape isn't pinned down here and guessing wrong would silently break
//! capability advertisement). Video (same adapter, different workflow graphs
//! per ADR-003 D61) and multi-minute-job checkpointing are explicitly out of
//! scope here — image jobs are single-shot and fast enough not to need it yet.

use super::{Backend, BackendError, Chunk, ChunkStream};
use crate::capability::{Capabilities, GpuVendor, Hardware, Modality, ModelRef, ToolsLevel};
use crate::job::Job;
use crate::ledger::Usage;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::time::{Duration, Instant};

pub struct ComfyUiBackend {
    /// e.g. `http://127.0.0.1:8188`.
    pub base_url: String,
    /// Checkpoint filename as ComfyUI's `models/checkpoints/` sees it, e.g.
    /// "flux1-dev-fp8.safetensors".
    pub checkpoint: String,
    /// How long to poll `/history` before giving up on a queued prompt.
    pub max_wait: Duration,
    client: reqwest::Client,
}

impl ComfyUiBackend {
    pub fn new(base_url: impl Into<String>, checkpoint: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            checkpoint: checkpoint.into(),
            max_wait: Duration::from_secs(180),
            client: reqwest::Client::new(),
        }
    }

    /// The standard ComfyUI txt2img graph (CheckpointLoaderSimple → two
    /// CLIPTextEncode → EmptyLatentImage → KSampler → VAEDecode → SaveImage),
    /// the same node wiring ComfyUI ships as its own default example workflow.
    fn workflow(&self, prompt: &str, negative: &str, opts: &ImageOpts) -> serde_json::Value {
        json!({
            "4": {
                "class_type": "CheckpointLoaderSimple",
                "inputs": { "ckpt_name": self.checkpoint }
            },
            "5": {
                "class_type": "EmptyLatentImage",
                "inputs": { "width": opts.width, "height": opts.height, "batch_size": 1 }
            },
            "6": {
                "class_type": "CLIPTextEncode",
                "inputs": { "text": prompt, "clip": ["4", 1] }
            },
            "7": {
                "class_type": "CLIPTextEncode",
                "inputs": { "text": negative, "clip": ["4", 1] }
            },
            "3": {
                "class_type": "KSampler",
                "inputs": {
                    "seed": opts.seed,
                    "steps": opts.steps,
                    "cfg": opts.cfg,
                    "sampler_name": "euler",
                    "scheduler": "normal",
                    "denoise": 1.0,
                    "model": ["4", 0],
                    "positive": ["6", 0],
                    "negative": ["7", 0],
                    "latent_image": ["5", 0]
                }
            },
            "8": {
                "class_type": "VAEDecode",
                "inputs": { "samples": ["3", 0], "vae": ["4", 2] }
            },
            "9": {
                "class_type": "SaveImage",
                "inputs": { "filename_prefix": "hive", "images": ["8", 0] }
            }
        })
    }

    async fn queue(&self, workflow: serde_json::Value) -> Result<String, BackendError> {
        #[derive(Deserialize)]
        struct QueueReply {
            prompt_id: String,
        }
        let client_id = uuid::Uuid::new_v4().to_string();
        let reply = self
            .client
            .post(format!("{}/prompt", self.base_url))
            .json(&json!({ "prompt": workflow, "client_id": client_id }))
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?
            .error_for_status()
            .map_err(|e| BackendError::Execution(format!("comfyui rejected prompt: {e}")))?
            .json::<QueueReply>()
            .await
            .map_err(|e| BackendError::Execution(format!("bad /prompt response: {e}")))?;
        Ok(reply.prompt_id)
    }

    /// Poll `/history/{id}` with linear backoff until an image artifact shows
    /// up under any node's `outputs.images`, or `max_wait` elapses.
    async fn poll_for_image(&self, prompt_id: &str) -> Result<HistoryImage, BackendError> {
        let started = Instant::now();
        let mut attempt: u64 = 0;
        loop {
            if started.elapsed() > self.max_wait {
                return Err(BackendError::Execution(format!(
                    "comfyui job {prompt_id} didn't finish within {:?}",
                    self.max_wait
                )));
            }
            let resp = self
                .client
                .get(format!("{}/history/{prompt_id}", self.base_url))
                .send()
                .await
                .map_err(|e| BackendError::Unavailable(e.to_string()))?
                .error_for_status()
                .map_err(|e| BackendError::Execution(e.to_string()))?
                .json::<serde_json::Map<String, serde_json::Value>>()
                .await
                .map_err(|e| BackendError::Execution(format!("bad /history response: {e}")))?;

            if let Some(entry) = resp.get(prompt_id) {
                if let Some(image) = first_image(entry) {
                    return Ok(image);
                }
            }
            attempt += 1;
            tokio::time::sleep(Duration::from_millis(500 * attempt.min(6))).await;
        }
    }
}

struct ImageOpts {
    width: u64,
    height: u64,
    steps: u64,
    cfg: f64,
    seed: u64,
}

impl ImageOpts {
    fn from_job(job: &Job) -> Self {
        let get_u64 = |k: &str, d: u64| job.input.get(k).and_then(|v| v.as_u64()).unwrap_or(d);
        let get_f64 = |k: &str, d: f64| job.input.get(k).and_then(|v| v.as_f64()).unwrap_or(d);
        Self {
            width: get_u64("width", 1024),
            height: get_u64("height", 1024),
            steps: get_u64("steps", 20),
            cfg: get_f64("cfg", 7.0),
            seed: get_u64("seed", rand_seed()),
        }
    }
}

fn rand_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

struct HistoryImage {
    filename: String,
    subfolder: String,
    kind: String,
}

/// Walk `history[prompt_id].outputs.*.images[0]` — ComfyUI keys outputs by the
/// SaveImage node's own numeric id, which this workflow always calls "9", but
/// a custom workflow could differ, so scan every node rather than hardcoding it.
fn first_image(entry: &serde_json::Value) -> Option<HistoryImage> {
    let outputs = entry.get("outputs")?.as_object()?;
    for node_output in outputs.values() {
        if let Some(images) = node_output.get("images").and_then(|v| v.as_array()) {
            if let Some(img) = images.first() {
                return Some(HistoryImage {
                    filename: img.get("filename")?.as_str()?.to_string(),
                    subfolder: img
                        .get("subfolder")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    kind: img
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("output")
                        .to_string(),
                });
            }
        }
    }
    None
}

#[async_trait]
impl Backend for ComfyUiBackend {
    fn name(&self) -> &'static str {
        "comfyui"
    }

    async fn capabilities(&self) -> Result<Capabilities, BackendError> {
        Ok(Capabilities {
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
            modalities: vec![Modality::Image],
            models: vec![ModelRef {
                id: self.checkpoint.clone(),
                modality: Modality::Image,
                backend: "comfyui".into(),
            }],
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
        let negative = job
            .input
            .get("negative_prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let opts = ImageOpts::from_job(job);

        let started = Instant::now();
        let workflow = self.workflow(prompt, negative, &opts);
        let prompt_id = self.queue(workflow).await?;
        let image = self.poll_for_image(&prompt_id).await?;

        let bytes = self
            .client
            .get(format!("{}/view", self.base_url))
            .query(&[
                ("filename", image.filename.as_str()),
                ("subfolder", image.subfolder.as_str()),
                ("type", image.kind.as_str()),
            ])
            .send()
            .await
            .map_err(|e| BackendError::Unavailable(e.to_string()))?
            .error_for_status()
            .map_err(|e| BackendError::Execution(e.to_string()))?
            .bytes()
            .await
            .map_err(|e| BackendError::Execution(format!("failed reading image bytes: {e}")))?;

        // Content-hashing and uploading to the artifact store (M9) is the
        // worker/tools layer's job (see `tools::run_artifact_put`), not the
        // backend's — the backend hands back raw bytes via a chunk's `text`
        // field is wrong for binary data, so for now this returns the local
        // temp path in `text` and lets the caller decide what to do with it.
        // TODO(M8 follow-on): plumb this through `artifact_put` directly once
        // Image jobs are wired into the worker's dispatch path, the same way
        // exec_wasm output already is.
        let tmp = std::env::temp_dir().join(format!("hive-comfyui-{prompt_id}.png"));
        tokio::fs::write(&tmp, &bytes)
            .await
            .map_err(|e| BackendError::Execution(format!("couldn't stage output image: {e}")))?;

        let usage = Usage {
            tokens_in: 0,
            tokens_out: 0,
            compute_seconds: started.elapsed().as_secs_f64(),
        };
        let stream = futures::stream::iter(vec![
            Ok(Chunk::text(tmp.display().to_string())),
            Ok(Chunk::done(usage)),
        ]);
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_image_scans_any_node_id() {
        let entry = json!({
            "outputs": {
                "9": { "images": [{ "filename": "hive_00001_.png", "subfolder": "", "type": "output" }] }
            }
        });
        let img = first_image(&entry).unwrap();
        assert_eq!(img.filename, "hive_00001_.png");
        assert_eq!(img.kind, "output");
    }

    #[test]
    fn first_image_none_when_no_outputs_yet() {
        let entry = json!({ "status": { "completed": false } });
        assert!(first_image(&entry).is_none());
    }

    #[test]
    fn image_opts_defaults_are_sane() {
        let job = Job {
            id: uuid::Uuid::new_v4(),
            kind: crate::job::JobKind::Inference,
            project_id: uuid::Uuid::nil(),
            card_id: None,
            parent: None,
            requirements: Default::default(),
            input: json!({ "prompt": "a fox" }),
            resume_from: None,
            created_at: chrono::Utc::now(),
        };
        let opts = ImageOpts::from_job(&job);
        assert_eq!(opts.width, 1024);
        assert_eq!(opts.steps, 20);
    }
}
