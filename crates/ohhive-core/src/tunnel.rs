//! Cloudflare Tunnel automation (ADR-013 D74, gateway option a). Moved here from
//! `apps/desktop/src-tauri/src/tunnel.rs` per ADR-018 decision 2/amendment 2026-09-09: the
//! native Swift shell's `ohhive-ffi` crate needs the exact same login/create/route/run flow as
//! the Tauri app, so it lives in the shared core instead of being duplicated. Locating and
//! bundling the `cloudflared` binary itself stays a per-shell concern (Tauri resource resolution
//! vs. a Swift app-bundle resource) -- both shells pass a resolved binary path into these
//! functions rather than this module knowing how to find one.
//!
//! Flow: `login` opens the printed URL and waits for `~/.cloudflared/cert.pem` to appear, then
//! `create` + `route_dns` + `write_config` produce a config the binary can `run` in the
//! background for as long as the regional server role is on (each shell ties this to its own
//! server_start/server_stop).

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cloudflared")
}

pub fn cert_path() -> PathBuf {
    config_dir().join("cert.pem")
}

pub fn logged_in() -> bool {
    cert_path().exists()
}

/// Run `cloudflared tunnel login`, open the URL it prints (via the caller-supplied opener,
/// typically shelling to `open`), and wait up to 5 minutes for `cert.pem` to land. Cloudflare's
/// own page confirms success; we only watch the filesystem.
pub async fn login(bin: &Path, open_url: impl Fn(String) + Send + 'static) -> Result<()> {
    let mut child = Command::new(bin)
        .args(["tunnel", "login"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn cloudflared tunnel login")?;
    let stdout = child.stdout.take().context("no stdout from cloudflared")?;
    let mut lines = BufReader::new(stdout).lines();
    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(start) = line.find("https://") {
                let url = line[start..].trim().to_string();
                if !url.is_empty() {
                    open_url(url);
                    break;
                }
            }
        }
    });
    let cert = cert_path();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    let outcome = loop {
        if cert.exists() {
            break Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            break Err(anyhow::anyhow!("timed out waiting for Cloudflare login"));
        }
        match child.try_wait() {
            Ok(Some(status)) if !cert.exists() => {
                break Err(anyhow::anyhow!(
                    "cloudflared exited ({status}) before login completed"
                ))
            }
            Ok(_) => tokio::time::sleep(Duration::from_millis(500)).await,
            Err(e) => break Err(e.into()),
        }
    };
    let _ = child.kill().await;
    outcome
}

pub struct CreatedTunnel {
    pub id: String,
    pub credentials_file: PathBuf,
}

/// Create a tunnel named `<name>-hive`, or look up the existing one if it already exists
/// (re-running Tunnel setup should never fail just because it ran before).
pub async fn create(bin: &Path, name: &str) -> Result<CreatedTunnel> {
    let tunnel_name = format!("{name}-hive");
    let out = Command::new(bin)
        .args(["tunnel", "create", &tunnel_name])
        .output()
        .await
        .context("run cloudflared tunnel create")?;
    if out.status.success() {
        let text = String::from_utf8_lossy(&out.stdout);
        let id = text
            .split("with id")
            .nth(1)
            .map(|s| s.trim().to_string())
            .context("could not parse tunnel id from cloudflared output")?;
        return Ok(CreatedTunnel {
            credentials_file: config_dir().join(format!("{id}.json")),
            id,
        });
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("already exists") {
        return lookup(bin, &tunnel_name).await;
    }
    bail!("cloudflared tunnel create failed: {stderr}");
}

async fn lookup(bin: &Path, tunnel_name: &str) -> Result<CreatedTunnel> {
    let out = Command::new(bin)
        .args(["tunnel", "list", "--name", tunnel_name, "-o", "json"])
        .output()
        .await
        .context("run cloudflared tunnel list")?;
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).context("parse `cloudflared tunnel list` output")?;
    let id = v
        .as_array()
        .and_then(|a| a.first())
        .and_then(|t| t.get("id"))
        .and_then(|s| s.as_str())
        .with_context(|| format!("tunnel {tunnel_name} not found"))?
        .to_string();
    Ok(CreatedTunnel {
        credentials_file: config_dir().join(format!("{id}.json")),
        id,
    })
}

/// Point `hostname` (must be a zone cloudflared's logged-in account controls) at the tunnel.
pub async fn route_dns(bin: &Path, name: &str, hostname: &str) -> Result<()> {
    let out = Command::new(bin)
        .args([
            "tunnel",
            "route",
            "dns",
            "--overwrite-dns",
            &format!("{name}-hive"),
            hostname,
        ])
        .output()
        .await
        .context("run cloudflared tunnel route dns")?;
    if !out.status.success() {
        bail!(
            "cloudflared tunnel route dns failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Write `~/.cloudflared/config.yml` — one ingress rule to the regional server's local port.
/// The default filename (not `config-<id>.yml`) matches `packaging/cloudflared-hive.service` and
/// docs/JOIN.md's manual instructions, and lets `cloudflared tunnel run` find it with no flags.
pub fn write_config(tunnel_id: &str, credentials_file: &Path, hostname: &str) -> Result<PathBuf> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).context("create ~/.cloudflared")?;
    let path = dir.join("config.yml");
    let yaml = format!(
        "tunnel: {tunnel_id}\ncredentials-file: {}\ningress:\n  - hostname: {hostname}\n    service: http://localhost:8790\n  - service: http_status:404\n",
        credentials_file.display()
    );
    std::fs::write(&path, yaml).context("write cloudflared config")?;
    Ok(path)
}

/// Start `cloudflared --no-autoupdate tunnel run` in the background (finds `~/.cloudflared/
/// config.yml` on its own). The caller owns the child and kills it when the regional server
/// role stops.
pub fn spawn_run(bin: &Path) -> Result<Child> {
    Command::new(bin)
        .args(["--no-autoupdate", "tunnel", "run"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("spawn cloudflared tunnel run")
}
