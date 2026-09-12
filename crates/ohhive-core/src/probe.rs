//! Hardware probe (ADR-010 registration step 2). Produces the `Hardware` half
//! of `Capabilities`; backends contribute models/modalities.
//!
//! GPU detection is best-effort via vendor tools so we don't link CUDA/Metal:
//! `nvidia-smi` for NVIDIA, `system_profiler` on macOS for Apple Silicon.
//! Bandwidth is left `None` here; a real speed test is the node app's call.

use crate::capability::{GpuVendor, Hardware};
use std::process::Command;

pub fn probe_hardware() -> Hardware {
    use sysinfo::{Disks, System};
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();

    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_default();
    let cpu_cores = sys.cpus().len() as u32;
    let ram_bytes = sys.total_memory();
    // Measured, not nominal (Project Halo lesson L3): a "24 GB" machine that's
    // someone's daily driver can have a fraction of that actually free. This
    // is read fresh on every probe/heartbeat, so it tracks real load over time
    // instead of a one-time capacity number.
    let ram_free_bytes = Some(sys.available_memory());
    let disk_free_bytes = Disks::new_with_refreshed_list()
        .iter()
        .map(|d| d.available_space())
        .max()
        .unwrap_or(0);

    let (gpu_vendor, gpu_model, vram_bytes, vram_free_bytes) =
        probe_gpu(&cpu_model, ram_bytes, ram_free_bytes);

    Hardware {
        cpu_model,
        cpu_cores,
        ram_bytes,
        ram_free_bytes,
        gpu_vendor,
        gpu_model,
        vram_bytes,
        vram_free_bytes,
        disk_free_bytes,
        upload_mbps: None,
        download_mbps: None,
    }
}

fn probe_gpu(
    cpu_model: &str,
    ram_bytes: u64,
    ram_free_bytes: Option<u64>,
) -> (GpuVendor, Option<String>, Option<u64>, Option<u64>) {
    // NVIDIA: nvidia-smi is present wherever the driver is. memory.free is a
    // live measurement from the driver itself — no estimation needed.
    if let Ok(out) = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(line) = s.lines().next() {
                let mut parts = line.split(',').map(|p| p.trim());
                let name = parts.next().unwrap_or("").to_string();
                let total_mib: u64 = parts.next().and_then(|m| m.parse().ok()).unwrap_or(0);
                let free_mib: Option<u64> = parts.next().and_then(|m| m.parse().ok());
                return (
                    GpuVendor::Nvidia,
                    Some(name),
                    Some(total_mib * 1024 * 1024),
                    free_mib.map(|m| m * 1024 * 1024),
                );
            }
        }
    }
    // Apple Silicon: unified memory — report the chip and treat RAM as VRAM
    // (the scheduler's min_vram check should use ~75% of it; see ADR-005 open
    // items). There's no cheap Metal API call here to measure free GPU memory
    // directly without linking Metal, so `vram_free_bytes` is derived from
    // measured `ram_free_bytes` using the same 75% headroom rule as the
    // nominal figure — an approximation, but a live one, unlike the old
    // always-75%-of-total number this replaces.
    if cfg!(target_os = "macos") && cpu_model.starts_with("Apple") {
        return (
            GpuVendor::Apple,
            Some(cpu_model.to_string()),
            Some(ram_bytes * 3 / 4),
            ram_free_bytes.map(|f| f * 3 / 4),
        );
    }
    // AMD via rocm-smi if present. rocm-smi's free-memory output isn't parsed
    // here yet — left None rather than guessed.
    if let Ok(out) = Command::new("rocm-smi")
        .args(["--showproductname", "--csv"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let name = s.lines().nth(1).map(|l| l.to_string());
            return (GpuVendor::Amd, name, None, None);
        }
    }
    (GpuVendor::None, None, None, None)
}
