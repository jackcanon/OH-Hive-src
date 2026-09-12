//! FFI wrapper for `hive_core::tunnel` (Cloudflare Tunnel automation, ADR-013 D74). Mirrors the
//! Tauri app's `tunnel_login`/`tunnel_setup` commands (`apps/desktop/src-tauri/src/lib.rs`)
//! one-for-one, minus binary discovery -- Swift resolves the bundled `cloudflared` resource
//! itself (there's no Tauri-style resource API on this side) and passes the path in, which this
//! module persists to `HIVE_CLOUDFLARED_BIN` so `server.rs`'s `server_start` can find it again on
//! its own without Swift re-supplying it every time the regional server role starts.

use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::{nodeconfig, tunnel};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(uniffi::Record, Clone)]
pub struct TunnelInfo {
    pub logged_in: bool,
    pub hostname: Option<String>,
    pub running: bool,
}

#[uniffi::export]
impl HiveNode {
    pub async fn tunnel_snapshot(self: Arc<Self>) -> TunnelInfo {
        TunnelInfo {
            logged_in: tunnel::logged_in(),
            hostname: std::env::var("HIVE_TUNNEL_HOSTNAME").ok(),
            running: self.tunnel_child.lock().await.is_some(),
        }
    }

    /// Opens the Cloudflare login page in the default browser and waits (up to 5 min) for
    /// `~/.cloudflared/cert.pem` to land. `bin_path` is the bundled `cloudflared` executable's
    /// path inside the app bundle's Resources.
    pub async fn tunnel_login(self: Arc<Self>, bin_path: String) -> Result<(), HiveError> {
        self.log("info", "opening Cloudflare login in your browser")
            .await;
        let bin = PathBuf::from(&bin_path);
        RUNTIME
            .spawn(async move {
                let opener = |url: String| {
                    let _ = std::process::Command::new("open").arg(url).spawn();
                };
                tunnel::login(&bin, opener).await
            })
            .await
            .map_err(|e| HiveError::Failed(format!("tunnel_login task panicked: {e}")))?
            .map_err(|e| HiveError::Failed(e.to_string()))?;
        let _ = nodeconfig::set("HIVE_CLOUDFLARED_BIN", &bin_path);
        std::env::set_var("HIVE_CLOUDFLARED_BIN", &bin_path);
        self.log("ok", "Cloudflare account connected").await;
        Ok(())
    }

    /// Create (or find) a tunnel named `<name>-hive`, route `hostname` to it, write its config,
    /// and save enough to node.env that `server_start` can bring the tunnel up and set the
    /// public URL. Does not start it running -- that happens the next time the regional server
    /// role starts (mirrors the Tauri app's `tunnel_setup`).
    pub async fn tunnel_setup(
        self: Arc<Self>,
        bin_path: String,
        name: String,
        hostname: String,
    ) -> Result<String, HiveError> {
        if !tunnel::logged_in() {
            return Err(HiveError::Failed(
                "connect your Cloudflare account first".into(),
            ));
        }
        let name = name.trim().to_lowercase();
        if name.is_empty() {
            return Err(HiveError::Failed(
                "give this machine a short name for the tunnel".into(),
            ));
        }
        let bin = PathBuf::from(&bin_path);
        let name2 = name.clone();
        let hostname2 = hostname.clone();
        let tunnel_id = RUNTIME
            .spawn(async move {
                let created = tunnel::create(&bin, &name2)
                    .await
                    .map_err(|e| HiveError::Failed(e.to_string()))?;
                tunnel::route_dns(&bin, &name2, &hostname2)
                    .await
                    .map_err(|e| HiveError::Failed(e.to_string()))?;
                tunnel::write_config(&created.id, &created.credentials_file, &hostname2)
                    .map_err(|e| HiveError::Failed(e.to_string()))?;
                Ok::<_, HiveError>(created.id)
            })
            .await
            .map_err(|e| HiveError::Failed(format!("tunnel_setup task panicked: {e}")))??;
        let public_url = format!("https://{hostname}");
        for (k, v) in [
            ("HIVE_TUNNEL_NAME", name.as_str()),
            ("HIVE_TUNNEL_ID", tunnel_id.as_str()),
            ("HIVE_TUNNEL_HOSTNAME", hostname.as_str()),
            ("HIVE_PUBLIC_URL", public_url.as_str()),
            ("HIVE_CLOUDFLARED_BIN", bin_path.as_str()),
        ] {
            nodeconfig::set(k, v).map_err(HiveError::from)?;
            std::env::set_var(k, v);
        }
        self.log("ok", format!("tunnel ready at {public_url}"))
            .await;
        Ok(public_url)
    }
}
