//! OH Hive desktop shell (ADR-010). Links `ohhive-core` in-process and exposes
//! it to the React UI over Tauri IPC. No second daemon.

use ohhive_core::backend::{mock::MockBackend, Backend};
use serde::Serialize;

/// Data for Preferences → About. Crediting Happy Jack Media and linking
/// This Is Not A Draft is a house rule and part of the definition of done.
#[derive(Serialize)]
pub struct AboutInfo {
    pub app_version: &'static str,
    pub core_version: &'static str,
    pub made_by: &'static str,
    pub made_by_url: &'static str,
    pub blog_name: &'static str,
    pub blog_url: &'static str,
}

#[tauri::command]
fn about() -> AboutInfo {
    AboutInfo {
        app_version: env!("CARGO_PKG_VERSION"),
        core_version: ohhive_core::VERSION,
        made_by: "Happy Jack Media",
        made_by_url: "https://happyjack.media",
        blog_name: "This Is Not A Draft",
        blog_url: "https://thisisnotadraft.com",
    }
}

#[tauri::command]
async fn probe_capabilities() -> Result<ohhive_core::Capabilities, String> {
    MockBackend.capabilities().await.map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![about, probe_capabilities])
        .run(tauri::generate_context!())
        .expect("error while running OH Hive");
}
