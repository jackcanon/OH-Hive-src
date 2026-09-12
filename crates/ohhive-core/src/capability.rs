//! Node capabilities — the scheduler's matching input (ADR-005 D40, ADR-003 D63).

use serde::{Deserialize, Serialize};

/// Output modality a node can produce. All five are v1 (ADR-003 D60).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Text,
    Code,
    Image,
    Video,
    Speech,
    Music,
}

/// What a card's agent loop may do on this node (ADR-006 D48). Default is
/// `SandboxedTools`; contributors may restrict to `InferenceOnly`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolsLevel {
    InferenceOnly,
    #[default]
    SandboxedTools,
}

/// GPU vendor, for backend selection and hardware-class pricing (ADR-002 open item).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Apple,
    Nvidia,
    Amd,
    Intel,
    None,
}

/// Hardware probe result (ADR-010 registration step 2).
///
/// `ram_bytes`/`vram_bytes` are nominal capacity (what the chip has). The
/// `_free_bytes` fields are measured at probe time (what's actually available
/// right now) — added 2026-09-10 per Project Halo lesson L3
/// (`docs/HALO-V2-INTEGRATION-LESSONS.md`): two identical machines can differ
/// 40%+ in real usable memory depending on what else is running, and a static
/// "~75% of RAM" estimate silently overcommits a loaded machine. Nominal
/// fields are kept for backward compat and as the fallback when a live
/// measurement isn't available (e.g. an older probe result); the scheduler
/// should prefer `_free_bytes` when present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hardware {
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub ram_bytes: u64,
    /// Measured free system RAM at probe time (`sysinfo`'s `available_memory`).
    pub ram_free_bytes: Option<u64>,
    pub gpu_vendor: GpuVendor,
    pub gpu_model: Option<String>,
    pub vram_bytes: Option<u64>,
    /// Measured free GPU memory at probe time: `nvidia-smi`'s `memory.free`
    /// on NVIDIA; derived from `ram_free_bytes` on Apple Silicon (unified
    /// memory, same 75% headroom rule as nominal `vram_bytes`); `None`
    /// elsewhere (AMD/no GPU, or the probe couldn't measure it).
    pub vram_free_bytes: Option<u64>,
    pub disk_free_bytes: u64,
    pub upload_mbps: Option<f32>,
    pub download_mbps: Option<f32>,
}

/// A model this node can serve, as advertised to the coordinator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelRef {
    /// Catalog id, e.g. "qwen3.6-27b-q4_k_m" or "flux.1-dev".
    pub id: String,
    pub modality: Modality,
    /// Which backend serves it: "llama_cpp", "mlx", "comfyui", "whisper", "tts".
    pub backend: String,
}

/// Everything the scheduler needs to decide whether a card fits this node.
/// Serialized into `hive.nodes.capabilities` (ADR-001) and sent on every
/// heartbeat when changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    pub hardware: Hardware,
    pub modalities: Vec<Modality>,
    pub models: Vec<ModelRef>,
    /// Whole-node internet opt-in, default false (ADR-006 D46).
    pub allow_internet: bool,
    pub tools_level: ToolsLevel,
    /// Present only when the node also acts as a regional server (ADR-004).
    pub storage_gb_offered: Option<u32>,
    /// Reserved for v2 distributed inference (ADR-003 D15). Always `None` in v1.
    pub shard_capable: Option<bool>,
}

/// What a card requires; matched against [`Capabilities`] by the coordinator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Requirements {
    pub modality: Option<Modality>,
    pub model_id: Option<String>,
    pub min_vram_bytes: Option<u64>,
    pub min_ram_bytes: Option<u64>,
    /// ADR-006 D47: only nodes with `allow_internet` may take this card.
    pub requires_internet: bool,
    pub tools_level: ToolsLevel,
}

impl Capabilities {
    /// Pure matching rule. Region preference is applied separately by the
    /// coordinator (capability-first, region-second — ADR-005).
    pub fn satisfies(&self, req: &Requirements) -> bool {
        if req.requires_internet && !self.allow_internet {
            return false;
        }
        if req.tools_level == ToolsLevel::SandboxedTools
            && self.tools_level == ToolsLevel::InferenceOnly
        {
            return false;
        }
        if let Some(m) = req.modality {
            if !self.modalities.contains(&m) {
                return false;
            }
        }
        if let Some(id) = &req.model_id {
            if !self.models.iter().any(|m| &m.id == id) {
                return false;
            }
        }
        if let Some(v) = req.min_vram_bytes {
            if self.hardware.vram_bytes.unwrap_or(0) < v {
                return false;
            }
        }
        if let Some(r) = req.min_ram_bytes {
            if self.hardware.ram_bytes < r {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(allow_internet: bool, tools: ToolsLevel) -> Capabilities {
        Capabilities {
            hardware: Hardware {
                cpu_model: "M4".into(),
                cpu_cores: 10,
                ram_bytes: 24 << 30,
                ram_free_bytes: Some(20 << 30),
                gpu_vendor: GpuVendor::Apple,
                gpu_model: None,
                vram_bytes: Some(16 << 30),
                vram_free_bytes: Some(15 << 30),
                disk_free_bytes: 100 << 30,
                upload_mbps: None,
                download_mbps: None,
            },
            modalities: vec![Modality::Text, Modality::Code],
            models: vec![ModelRef {
                id: "qwen3.6".into(),
                modality: Modality::Text,
                backend: "llama_cpp".into(),
            }],
            allow_internet,
            tools_level: tools,
            storage_gb_offered: None,
            shard_capable: None,
        }
    }

    #[test]
    fn internet_is_never_granted_to_opted_out_nodes() {
        let req = Requirements {
            requires_internet: true,
            ..Default::default()
        };
        assert!(!caps(false, ToolsLevel::SandboxedTools).satisfies(&req));
        assert!(caps(true, ToolsLevel::SandboxedTools).satisfies(&req));
    }

    #[test]
    fn inference_only_nodes_reject_tool_cards() {
        let req = Requirements {
            tools_level: ToolsLevel::SandboxedTools,
            ..Default::default()
        };
        assert!(!caps(false, ToolsLevel::InferenceOnly).satisfies(&req));
        let req = Requirements {
            tools_level: ToolsLevel::InferenceOnly,
            ..Default::default()
        };
        assert!(caps(false, ToolsLevel::InferenceOnly).satisfies(&req));
    }

    #[test]
    fn modality_and_model_must_match() {
        let c = caps(false, ToolsLevel::SandboxedTools);
        assert!(c.satisfies(&Requirements {
            modality: Some(Modality::Text),
            ..Default::default()
        }));
        assert!(!c.satisfies(&Requirements {
            modality: Some(Modality::Video),
            ..Default::default()
        }));
        assert!(!c.satisfies(&Requirements {
            model_id: Some("flux".into()),
            ..Default::default()
        }));
    }
}
