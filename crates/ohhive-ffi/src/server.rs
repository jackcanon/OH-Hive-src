//! FFI wrapper for the regional-server role (`hive_server::serve`, in-process). Mirrors the
//! Tauri app's `server_start`/`server_stop`/`ServerView` (`apps/desktop/src-tauri/src/lib.rs`)
//! one-for-one, minus Cloudflare Tunnel lifecycle -- that's ADR-018 task #71, not this one. For
//! now this only supports a manually-configured public URL (a Cloudflare Tunnel hostname set up
//! in Terminal, or a real public IP), the same fallback path `Server.tsx` uses when its bundled
//! tunnel isn't available.

use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::hub::HubClient;
use hive_core::nodeconfig;
use hive_core::tunnel;
use hive_server::{ServeOptions, ServerStatus};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::watch;

fn env_flag_or(k: &str, d: &str) -> String {
    std::env::var(k)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| d.to_string())
}

#[derive(uniffi::Record, Clone)]
pub struct ServerInfo {
    pub running: bool,
    pub registered: bool,
    pub coordinator: bool,
    pub coordinator_name: Option<String>,
    pub blobs: u64,
    pub used_bytes: u64,
    pub last_backup: Option<String>,
    pub public_url: String,
    pub storage_gb: u32,
    pub tier: String,
    /// Named `operator_name`, not `operator` -- `operator` is a Swift keyword and UniFFI's Swift
    /// codegen doesn't backtick-escape reserved words in generated record field names.
    pub operator_name: String,
    pub listen: String,
    pub data_dir: String,
}

async fn view(node: &HiveNode, status: &ServerStatus) -> ServerInfo {
    ServerInfo {
        running: node.server_stop.lock().await.is_some(),
        registered: status.registered.load(Ordering::Relaxed),
        coordinator: status.coordinator.load(Ordering::Relaxed),
        coordinator_name: status.coordinator_name.lock().await.clone(),
        blobs: status.blobs.load(Ordering::Relaxed),
        used_bytes: status.used_bytes.load(Ordering::Relaxed),
        last_backup: status.last_backup.lock().await.clone(),
        public_url: env_flag_or("HIVE_PUBLIC_URL", ""),
        storage_gb: env_flag_or("HIVE_STORAGE_GB", "50").parse().unwrap_or(50),
        tier: env_flag_or("HIVE_TIER", "primary"),
        operator_name: env_flag_or("HIVE_OPERATOR", "volunteer"),
        listen: env_flag_or("HIVE_LISTEN", "0.0.0.0:8790"),
        data_dir: std::env::var("HIVE_DATA_DIR")
            .unwrap_or_else(|_| hive_server::default_data_dir().display().to_string()),
    }
}

/// `HIVE_DATA_DIR` gets its own check -- a typo or an unmounted drive should surface here, not
/// as a confusing failure deep inside `hive_server::serve`. Mirrors the Tauri app's
/// `validate_data_dir`.
fn validate_data_dir(path: &str) -> Result<(), HiveError> {
    let dir = std::path::Path::new(path.trim());
    std::fs::create_dir_all(dir)
        .map_err(|e| HiveError::Failed(format!("can't use {path} as storage: {e}")))?;
    let probe = dir.join(".ohhive-write-test");
    std::fs::write(&probe, b"ok")
        .map_err(|e| HiveError::Failed(format!("{path} isn't writable by OH Hive: {e}")))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

#[uniffi::export]
impl HiveNode {
    /// Regional-server + reachability snapshot. Safe to call repeatedly, same as `assess()`.
    pub async fn server_snapshot(self: Arc<Self>) -> ServerInfo {
        let status = self.server_status.clone();
        view(&self, &status).await
    }

    /// Sets `HIVE_DATA_DIR` after validating it's writable. Separate from the generic
    /// `set_config` because a bad storage path deserves a clearer error than "wrote a string to
    /// a file" -- mirrors the Tauri app's dedicated check in `set_config`.
    pub fn set_data_dir(&self, path: String) -> Result<String, HiveError> {
        if !path.trim().is_empty() {
            validate_data_dir(&path)?;
        }
        let p = nodeconfig::set("HIVE_DATA_DIR", &path).map_err(HiveError::from)?;
        std::env::set_var("HIVE_DATA_DIR", path.trim());
        Ok(p.display().to_string())
    }

    pub async fn server_start(
        self: Arc<Self>,
        public_url: Option<String>,
        storage_gb: Option<u32>,
        tier: Option<String>,
    ) -> Result<(), HiveError> {
        if self.server_stop.lock().await.is_some() {
            return Ok(());
        }
        RUNTIME
            .spawn(async move {
                nodeconfig::export_env();
                if let Some(u) = public_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|u| !u.is_empty())
                {
                    nodeconfig::set("HIVE_PUBLIC_URL", u).map_err(HiveError::from)?;
                    std::env::set_var("HIVE_PUBLIC_URL", u);
                }
                if let Some(g) = storage_gb {
                    nodeconfig::set("HIVE_STORAGE_GB", &g.to_string()).map_err(HiveError::from)?;
                    std::env::set_var("HIVE_STORAGE_GB", g.to_string());
                }
                if let Some(t) = tier
                    .as_deref()
                    .filter(|t| *t == "primary" || *t == "standby")
                {
                    nodeconfig::set("HIVE_TIER", t).map_err(HiveError::from)?;
                    std::env::set_var("HIVE_TIER", t);
                }
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let key = cfg
                    .node_key
                    .clone()
                    .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
                let opts = ServeOptions {
                    public_url: env_flag_or("HIVE_PUBLIC_URL", ""),
                    listen: env_flag_or("HIVE_LISTEN", "0.0.0.0:8790"),
                    data_dir: std::env::var("HIVE_DATA_DIR")
                        .ok()
                        .map(std::path::PathBuf::from),
                    storage_gb: env_flag_or("HIVE_STORAGE_GB", "50").parse().unwrap_or(50),
                    operator: env_flag_or("HIVE_OPERATOR", "volunteer"),
                    tier: env_flag_or("HIVE_TIER", "primary"),
                    region: cfg.region.clone(),
                    max_upload_mb: env_flag_or("HIVE_MAX_UPLOAD_MB", "512")
                        .parse()
                        .unwrap_or(512),
                    backup_recipient: std::env::var("HIVE_BACKUP_RECIPIENT")
                        .ok()
                        .filter(|s| !s.trim().is_empty()),
                    backup_hour_utc: env_flag_or("HIVE_BACKUP_HOUR_UTC", "9")
                        .parse()
                        .unwrap_or(9),
                };
                if opts.public_url.is_empty() {
                    return Err(HiveError::Failed(
                        "set the public URL first (a Cloudflare Tunnel hostname set up in \
                         Terminal, or http://<public-ip>:8790)"
                            .into(),
                    ));
                }
                // If Tunnel setup has run (HIVE_TUNNEL_ID + HIVE_CLOUDFLARED_BIN both set), bring
                // the tunnel up alongside the server so the public URL it configured is actually
                // reachable. Not fatal if it fails to start -- a manually-run cloudflared, or a
                // real public IP, still works; this call only needed opts.public_url. Mirrors the
                // Tauri app's server_start.
                if self.tunnel_child.lock().await.is_none() {
                    if let (Ok(bin), Ok(_id)) = (
                        std::env::var("HIVE_CLOUDFLARED_BIN"),
                        std::env::var("HIVE_TUNNEL_ID"),
                    ) {
                        match tunnel::spawn_run(std::path::Path::new(&bin)) {
                            Ok(child) => {
                                *self.tunnel_child.lock().await = Some(child);
                                self.log("ok", "cloudflare tunnel connecting").await;
                            }
                            Err(e) => {
                                self.log("error", format!("tunnel did not start: {e}"))
                                    .await
                            }
                        }
                    }
                }
                let (stop_tx, mut stop_rx) = watch::channel(false);
                *self.server_stop.lock().await = Some(stop_tx);
                let _ = nodeconfig::set("HIVE_SERVER_ENABLED", "1");
                std::env::set_var("HIVE_SERVER_ENABLED", "1");
                let status = self.server_status.clone();
                let hub = Arc::new(HubClient::new(&cfg.hub_url, &cfg.anon_key, key));
                self.log(
                    "ok",
                    format!(
                        "regional server starting at {} (storage {} GB, {})",
                        opts.public_url, opts.storage_gb, opts.tier
                    ),
                )
                .await;
                let this = self.clone();
                RUNTIME.spawn(async move {
                    let stop = async move {
                        while !*stop_rx.borrow() {
                            if stop_rx.changed().await.is_err() {
                                break;
                            }
                        }
                    };
                    let r = hive_server::serve(&cfg, hub, opts, stop, status.clone()).await;
                    match r {
                        Ok(()) => {
                            this.log("info", "regional server stopped \u{2014} checked out")
                                .await
                        }
                        Err(e) => {
                            this.log("error", format!("regional server stopped: {e:#}"))
                                .await
                        }
                    }
                    status.registered.store(false, Ordering::Relaxed);
                    status.coordinator.store(false, Ordering::Relaxed);
                    *this.server_stop.lock().await = None;
                    this.notify_changed().await;
                });
                Ok(())
            })
            .await
            .map_err(|e| HiveError::Failed(format!("server_start task panicked: {e}")))?
    }

    pub async fn server_stop(&self, forget: bool) {
        if let Some(tx) = self.server_stop.lock().await.as_ref() {
            let _ = tx.send(true);
            self.log("info", "stopping regional server").await;
        }
        if let Some(mut child) = self.tunnel_child.lock().await.take() {
            let _ = child.kill().await;
        }
        if forget {
            let _ = nodeconfig::set("HIVE_SERVER_ENABLED", "0");
            std::env::set_var("HIVE_SERVER_ENABLED", "0");
        }
    }
}
