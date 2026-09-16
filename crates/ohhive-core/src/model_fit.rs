//! Stable admission advertisement, not a live allocation guarantee.
use crate::capability::{GpuVendor, Hardware, ModelRef};

/// Reserve one quarter for runtime, context and other memory use. Avoid overflow.
pub fn with_headroom(bytes: u64) -> u64 {
    bytes / 4 * 3
}

pub fn memory_budget(hardware: &Hardware) -> u64 {
    if hardware.gpu_vendor == GpuVendor::Apple {
        // Apple probe already reports 75% of unified RAM as nominal VRAM.
        hardware
            .vram_bytes
            .unwrap_or_else(|| with_headroom(hardware.ram_bytes))
    } else {
        with_headroom(hardware.vram_bytes.unwrap_or(hardware.ram_bytes))
    }
}

/// Conservative name fallback, never confuse a model version (qwen3.8) with parameters.
/// Unspecified quantization assumes two bytes/parameter; metadata should be preferred.
pub fn estimate_weights(id: &str) -> Option<u64> {
    let id = id.to_ascii_lowercase();
    // MoE names like 8x7b need total parameters, not just the active/expert size.
    let parameters = id.split(['/', ':', '-', '_']).find_map(|part| {
        let n = part.strip_suffix('b')?;
        if n.contains('x') {
            return None;
        }
        n.parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0)
    })?;
    let bytes_per_parameter = if id.contains("q4") {
        0.65
    } else if id.contains("q5") {
        0.8
    } else if id.contains("q6") {
        0.95
    } else if id.contains("q8") {
        1.15
    } else if id.contains("f32") || id.contains("fp32") {
        4.0
    } else {
        2.0
    };
    let size = parameters * 1_000_000_000.0 * bytes_per_parameter;
    (size < u64::MAX as f64).then_some(size.ceil() as u64)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct FitReport {
    pub dropped: usize,
    pub unknown: usize,
}

pub fn filter_models(hardware: &Hardware, models: &mut Vec<ModelRef>) -> FitReport {
    let budget = memory_budget(hardware);
    let mut report = FitReport::default();
    models.retain(|model| {
        let size = model.size_bytes.filter(|n| *n > 0).or_else(|| estimate_weights(&model.id));
        match size {
            Some(size) if budget > 0 && size > budget => {
                tracing::warn!(model = %model.id, weight_bytes = size, budget_bytes = budget,
                    "not advertising model: weights exceed memory budget after headroom");
                report.dropped += 1;
                false
            }
            None => {
                tracing::warn!(model = %model.id, "advertising model with unknown weight size; memory fit is unverified");
                report.unknown += 1;
                true
            }
            Some(_) if budget == 0 => {
                tracing::warn!(model = %model.id, "advertising model with unknown hardware memory; memory fit is unverified");
                report.unknown += 1;
                true
            }
            _ => true,
        }
    });
    if report.unknown > 0 {
        tracing::warn!(
            unknown_models = report.unknown,
            "model-fit advertisement includes unverified models"
        );
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hardware(vendor: GpuVendor, ram: u64, vram: Option<u64>) -> Hardware {
        Hardware {
            cpu_model: String::new(),
            cpu_cores: 1,
            ram_bytes: ram,
            ram_free_bytes: Some(1),
            gpu_vendor: vendor,
            gpu_model: None,
            vram_bytes: vram,
            vram_free_bytes: Some(1),
            disk_free_bytes: 0,
            upload_mbps: None,
            download_mbps: None,
        }
    }
    fn model(id: &str, size: Option<u64>) -> ModelRef {
        ModelRef {
            id: id.into(),
            backend: "llama_cpp".into(),
            modality: crate::capability::Modality::Text,
            size_bytes: size,
        }
    }
    #[test]
    fn jotunheim_drops_27b_and_keeps_8b_without_double_headroom() {
        let hw = hardware(GpuVendor::Apple, 16 << 30, Some(12 << 30));
        let mut models = vec![
            model("qwen3.8:27b", Some(16_000_000_000)),
            model("llama-8b-q4", None),
        ];
        assert_eq!(filter_models(&hw, &mut models).dropped, 1);
        assert_eq!(models[0].id, "llama-8b-q4");
        assert_eq!(memory_budget(&hw), 12 << 30);
    }
    #[test]
    fn heimdall_uses_vram_and_all_dropped_means_empty() {
        let hw = hardware(GpuVendor::Nvidia, 67_000_000_000, Some(12_900_000_000));
        let mut models = vec![model("qwen3.8:27b", Some(16_000_000_000))];
        assert_eq!(filter_models(&hw, &mut models).dropped, 1);
        assert!(models.is_empty());
    }
    #[test]
    fn unknown_size_is_kept_and_emits_warning() {
        use std::sync::{Arc, Mutex};
        #[derive(Clone)]
        struct Buffer(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buffer = Buffer(Arc::new(Mutex::new(Vec::new())));
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let mut models = vec![model("custom-unknown", None)];
        let report = tracing::subscriber::with_default(subscriber, || {
            filter_models(&hardware(GpuVendor::None, 16 << 30, None), &mut models)
        });
        assert_eq!(report.unknown, 1);
        assert_eq!(models.len(), 1);
        let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert!(log.contains("unknown weight size"));
        assert!(log.contains("custom-unknown"));
    }
    #[test]
    fn reported_size_wins_and_old_wire_models_decode() {
        let old: ModelRef =
            serde_json::from_str(r#"{"id":"custom","backend":"llama_cpp","modality":"text"}"#)
                .unwrap();
        assert_eq!(old.size_bytes, None);
        let mut models = vec![model("model-27b-q4", Some(1_000_000_000))];
        assert_eq!(
            filter_models(&hardware(GpuVendor::None, 4 << 30, None), &mut models).dropped,
            0
        );
        assert!(estimate_weights("qwen3.8:latest").is_none());
        assert!(estimate_weights("mixtral-8x7b-q4").is_none());
        assert!(estimate_weights("qwen-27b-q4_k_m").unwrap() > 12 << 30);
    }
}
