//! Node config: `~/.config/ohhive/node.env` (mode 0600), KEY=VALUE lines.
//! Env vars override the file. Keys:
//!   HIVE_HUB_URL, HIVE_HUB_ANON_KEY, HIVE_NODE_KEY, HIVE_LLAMA_URL, HIVE_REGION,
//!   HIVE_ALLOW_INTERNET, HIVE_TOOLS_LEVEL, HIVE_WHISPER_URL, HIVE_WHISPER_MODEL,
//!   HIVE_COMFYUI_URL, HIVE_COMFYUI_CHECKPOINT
//!
//! Unlike `HIVE_LLAMA_URL` (defaults to Ollama's local port — a text backend is
//! the common case), the M8 media backends have no default: most nodes don't
//! run a whisper.cpp server or ComfyUI instance, so `capabilities()` only
//! probes them when a URL is actually configured (`hive set HIVE_WHISPER_URL …`),
//! rather than spending a heartbeat's worth of latency on a connection that
//! will fail on every node that hasn't opted in.

use crate::capability::ToolsLevel;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

pub const DEFAULT_HUB_URL: &str = "https://pxfbnuxcnerulbvbmowz.supabase.co";
pub const DEFAULT_ANON_KEY: &str = "sb_publishable_VjfocwhBAykEFEllo6U3RQ_e-BdMcme";

/// The platform's private-fleet enrollment trust triplet: an Ed25519 **public** verification
/// key plus its issuer and key-id labels, used to check the signed approval a fleet owner
/// issues from ohghive.com when a new device asks to join (see
/// docs/SIF-PRIVATE-FLEET-ENROLLMENT-2026-09-15.md). This is not a per-machine secret --
/// every installation trusts the same platform signer, exactly like DEFAULT_HUB_URL/
/// DEFAULT_ANON_KEY above -- so it is safe, and necessary, to compile in. Without a default
/// here, a fresh install has no way to complete "Request to join" without someone hand-editing
/// its local node.env first, which defeats the point of a self-serve onboarding flow.
pub const DEFAULT_PRIVATE_FLEET_ISSUER: &str = "https://ohghive.com";
pub const DEFAULT_PRIVATE_FLEET_KEY_ID: &str = "hive-private-fleet-2026-09";
pub const DEFAULT_PRIVATE_FLEET_PUBLIC_KEY: &str = "rr69aY892zh5AptSRcnEGhpEiOKzm-VGzkZqjl5cy6Y";

pub fn path() -> PathBuf {
    config_base().join("ohhive").join("node.env")
}

/// The directory that holds `ohhive/node.env`, `ohhive/vault-host.sqlite3`, and everything else
/// under it -- normally `dirs::config_dir()` (`~/Library/Application Support` on macOS), but
/// redirected to a different machine-local home when `<normal ohhive dir>/hub-home` names one.
///
/// Why this exists: a machine that also runs the hub-serving process under a separate `HOME`
/// (see the `media.happyjack.hive-hub` launchd job on Asgard, `HOME=/Users/jack/hive-hub-home`)
/// has two different `ohhive` directories -- the hub's real one, and whatever the GUI/CLI would
/// open under the machine's ordinary login session. Without this redirect, launching Loki's Den
/// normally on that machine silently opens an empty or unrelated local vault instead of the real
/// shared Library (found 2026-09-27, while scoping the collection rename/delete GUI feature).
/// The fix is a one-line marker file, written once by hand on the hub machine only -- not part of
/// normal pairing/enrollment, and a no-op (falls through to the normal path) everywhere else.
fn config_base() -> PathBuf {
    let normal = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config_base_for(normal, std::env::var_os("HIVE_CONFIG_BASE"))
}

fn config_base_for(normal: PathBuf, explicit: Option<std::ffi::OsString>) -> PathBuf {
    // Service processes may have a separate paired identity from the desktop hub.
    // Pin their existing config directory without changing the machine-wide redirect.
    if let Some(base) = explicit.filter(|s| !s.is_empty()) {
        return PathBuf::from(base);
    }
    resolve_hub_home_redirect(&normal).unwrap_or(normal)
}

fn resolve_hub_home_redirect(normal_config_base: &std::path::Path) -> Option<PathBuf> {
    let marker = normal_config_base.join("ohhive").join("hub-home");
    let target = std::fs::read_to_string(&marker).ok()?;
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    // Reproduce the same "<HOME>/Library/Application Support" shape `dirs::config_dir()` would
    // compute if $HOME were `target` -- matching how the hub-serving launchd job gets there today
    // (it just sets HOME directly; this achieves the same result for a process that can't).
    Some(
        PathBuf::from(target)
            .join("Library")
            .join("Application Support"),
    )
}

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub hub_url: String,
    pub anon_key: String,
    pub node_key: Option<String>,
    pub llama_url: String,
    /// Set only when this node runs a whisper.cpp `whisper-server` (M8).
    pub whisper_url: Option<String>,
    pub whisper_model: Option<String>,
    /// Set only when this node runs ComfyUI in API mode (M8).
    pub comfyui_url: Option<String>,
    pub comfyui_checkpoint: Option<String>,
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
        whisper_url: get("HIVE_WHISPER_URL"),
        whisper_model: get("HIVE_WHISPER_MODEL"),
        comfyui_url: get("HIVE_COMFYUI_URL"),
        comfyui_checkpoint: get("HIVE_COMFYUI_CHECKPOINT"),
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

/// Reads one `HIVE_*` key that isn't part of `NodeConfig` (env override, then the file), with
/// the same precedence `load()` uses for its own fields. For settings that are real but don't
/// belong on the shared struct every caller sees -- e.g. the desktop app's local vault reader
/// credential (`HIVE_VAULT_SELF_KEY`, `crates/ohhive-ffi/src/local_hub.rs`), which nothing
/// outside that one feature needs to know about.
pub fn get_extra(key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(key) {
        if !v.trim().is_empty() {
            return Some(v);
        }
    }
    let text = std::fs::read_to_string(path()).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                let v = v.trim().trim_matches('"');
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Like `get_extra`, but for the private-fleet trust triplet specifically: falls back to the
/// compiled-in `DEFAULT_PRIVATE_FLEET_*` constants instead of `None`, since these three are
/// the same for every installation. `HIVE_PRIVATE_FLEET_*` in the environment or node.env
/// still overrides it -- needed for key rotation or a non-production deployment -- this only
/// changes what happens when nothing local was ever configured.
pub fn private_fleet_trust_defaults() -> (String, String, String) {
    (
        get_extra("HIVE_PRIVATE_FLEET_ISSUER")
            .unwrap_or_else(|| DEFAULT_PRIVATE_FLEET_ISSUER.into()),
        get_extra("HIVE_PRIVATE_FLEET_KEY_ID")
            .unwrap_or_else(|| DEFAULT_PRIVATE_FLEET_KEY_ID.into()),
        get_extra("HIVE_PRIVATE_FLEET_PUBLIC_KEY")
            .unwrap_or_else(|| DEFAULT_PRIVATE_FLEET_PUBLIC_KEY.into()),
    )
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

#[cfg(test)]
mod hub_home_redirect_tests {
    use super::resolve_hub_home_redirect;
    use std::path::PathBuf;

    #[test]
    fn service_config_override_preserves_identity_despite_hub_redirect() {
        let dir = ScratchDir::new("service");
        std::fs::create_dir_all(dir.0.join("ohhive")).unwrap();
        std::fs::write(dir.0.join("ohhive/hub-home"), "/other-hub-home").unwrap();
        assert_eq!(super::config_base_for(dir.0.clone(), Some(dir.0.clone().into_os_string())), dir.0);
        assert_eq!(super::config_base_for(dir.0.clone(), Some("".into())),
            PathBuf::from("/other-hub-home/Library/Application Support"));
    }

    /// A scratch directory under the OS temp dir, unique per test run; removed on drop so
    /// parallel test threads (each calling this once) never collide or leak files.
    struct ScratchDir(PathBuf);
    impl ScratchDir {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "ohhive-nodeconfig-test-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn no_marker_file_means_no_redirect() {
        let dir = ScratchDir::new("none");
        assert!(resolve_hub_home_redirect(&dir.0).is_none());
    }

    #[test]
    fn empty_marker_file_means_no_redirect() {
        let dir = ScratchDir::new("empty");
        std::fs::create_dir_all(dir.0.join("ohhive")).unwrap();
        std::fs::write(dir.0.join("ohhive").join("hub-home"), "   \n").unwrap();
        assert!(resolve_hub_home_redirect(&dir.0).is_none());
    }

    #[test]
    fn marker_file_redirects_to_that_home_library_application_support() {
        let dir = ScratchDir::new("redirect");
        std::fs::create_dir_all(dir.0.join("ohhive")).unwrap();
        std::fs::write(
            dir.0.join("ohhive").join("hub-home"),
            "/Users/jack/hive-hub-home\n",
        )
        .unwrap();
        assert_eq!(
            resolve_hub_home_redirect(&dir.0).unwrap(),
            PathBuf::from("/Users/jack/hive-hub-home/Library/Application Support")
        );
    }
}
