//! Cloudflare Tunnel automation (ADR-013 D74, gateway option a). The login/create/route/run
//! flow moved to `hive_core::tunnel` (ADR-018 decision 2/amendment 2026-09-09) so the native
//! Swift shell's regional-server role shares it instead of duplicating it. This file keeps only
//! what's genuinely Tauri-specific: resolving the bundled `cloudflared` binary from Tauri's
//! resource directory, and the `TunnelView` shape this app's frontend expects.

pub use hive_core::tunnel::*;

use serde::Serialize;

/// Resolve the bundled binary for this build (Tauri resource), or `None` if this build has none
/// (e.g. a dev build on a platform build.rs doesn't fetch for).
pub fn bundled_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    let rel = format!(
        "resources/cloudflared-{}-apple-darwin",
        std::env::consts::ARCH
    );
    app.path()
        .resolve(&rel, tauri::path::BaseDirectory::Resource)
        .ok()
        .filter(|p| p.exists())
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct TunnelView {
    /// The bundled binary is present in this build.
    pub available: bool,
    /// `cloudflared tunnel login` has been completed at least once on this machine.
    pub logged_in: bool,
    pub hostname: Option<String>,
    pub running: bool,
}
