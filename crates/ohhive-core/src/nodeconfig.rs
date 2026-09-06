//! Node config: `~/.config/ohhive/node.env` (mode 0600), KEY=VALUE lines.
//! Env vars override the file. Keys:
//!   HIVE_HUB_URL, HIVE_HUB_ANON_KEY, HIVE_NODE_KEY, HIVE_LLAMA_URL, HIVE_REGION,
//!   HIVE_ALLOW_INTERNET, HIVE_TOOLS_LEVEL

use crate::capability::ToolsLevel;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

pub const DEFAULT_HUB_URL: &str = "https://pxfbnuxcnerulbvbmowz.supabase.co";
pub const DEFAULT_ANON_KEY: &str = "sb_publishable_VjfocwhBAykEFEllo6U3RQ_e-BdMcme";

pub fn path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ohhive")
        .join("node.env")
}

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub hub_url: String,
    pub anon_key: String,
    pub node_key: Option<String>,
    pub llama_url: String,
    pub region: Option<String>,
    /// ADR-006 D46: whole-node internet opt-in, default false. Read from node.env so it
    /// survives restarts instead of being reset by the check-in payload.
    pub allow_internet: bool,
    /// ADR-006 D48: default sandboxed tools; contributors may restrict to inference-only.
    pub tools_level: ToolsLevel,
}

/// Export every `HIVE_*` key in node.env into the process environment (without overriding
/// values already set), so clap `env = "HIVE_…"` flags and anything else see them. Call before
/// parsing the CLI.
pub fn export_env() {
    if let Ok(text) = std::fs::read_to_string(path()) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim(), v.trim().trim_matches('"'));
                if k.starts_with("HIVE_")
                    && !v.is_empty()
                    && std::env::var(k)
                        .map(|e| e.trim().is_empty())
                        .unwrap_or(true)
                {
                    std::env::set_var(k, v);
                }
            }
        }
    }
}

pub fn load() -> Result<NodeConfig> {
    let mut kv: HashMap<String, String> = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(path()) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                kv.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
            }
        }
    }
    let get = |k: &str| {
        std::env::var(k)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| kv.get(k).cloned().filter(|v| !v.trim().is_empty()))
    };
    Ok(NodeConfig {
        hub_url: get("HIVE_HUB_URL").unwrap_or_else(|| DEFAULT_HUB_URL.into()),
        anon_key: get("HIVE_HUB_ANON_KEY").unwrap_or_else(|| DEFAULT_ANON_KEY.into()),
        node_key: get("HIVE_NODE_KEY"),
        llama_url: get("HIVE_LLAMA_URL").unwrap_or_else(|| "http://127.0.0.1:11434".into()),
        region: get("HIVE_REGION"),
        allow_internet: get("HIVE_ALLOW_INTERNET")
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false),
        tools_level: match get("HIVE_TOOLS_LEVEL").as_deref() {
            Some("inference_only") => ToolsLevel::InferenceOnly,
            _ => ToolsLevel::SandboxedTools,
        },
    })
}

/// Write/replace one key in the config file, creating it with 0600.
pub fn set(key: &str, value: &str) -> Result<PathBuf> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let existing = std::fs::read_to_string(&p).unwrap_or_default();
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| !l.trim_start().starts_with(&format!("{key}=")))
        .map(String::from)
        .collect();
    lines.push(format!("{key}={value}"));
    std::fs::write(&p, lines.join("\n") + "\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(p)
}

pub fn require_node_key(cfg: &NodeConfig) -> Result<String> {
    cfg.node_key.clone().context(format!(
        "no node key. Run `hive pair` (or set HIVE_NODE_KEY in {})",
        path().display()
    ))
}
