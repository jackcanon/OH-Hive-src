//! Optional, explicitly invoked owner setup. Reuses existing Cloudflare login/create/DNS
//! automation, but NEVER overwrites the community regional server's config.yml or process.
use super::{rejected, Result};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::process::{Child, Command};

pub fn write_config(
    path: &Path,
    tunnel_id: &str,
    credentials: &Path,
    hostname: &str,
    local: SocketAddr,
) -> Result<PathBuf> {
    if !local.ip().is_loopback() {
        return Err(rejected("tunnel target must be loopback"));
    }
    if hostname.is_empty() || hostname.contains(['/', ':', '\n', ' ']) {
        return Err(rejected("invalid tunnel hostname"));
    }
    let value = serde_json::json!({"tunnel":tunnel_id,"credentials-file":credentials,"ingress":[{"hostname":hostname,"service":format!("http://{local}")},{"service":"http_status:404"}]});
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut f = options.open(path).map_err(|_| {
        rejected("cannot create separate local tunnel config; existing files are preserved")
    })?;
    f.write_all(&serde_json::to_vec_pretty(&value).map_err(|_| rejected("invalid tunnel config"))?)
        .map_err(|_| rejected("cannot write tunnel config"))?;
    Ok(path.to_path_buf())
}
/// Explicit remote setup: uses the existing authenticated cloudflared installation.
/// Caller must choose an unused hostname and private config path. No login/account data is
/// touched by LocalHub itself, and this function is never called by serving/pairing a LAN hub.
pub async fn provision(
    bin: &Path,
    name: &str,
    hostname: &str,
    local: SocketAddr,
    config: &Path,
) -> Result<PathBuf> {
    if config.exists() || !local.ip().is_loopback() {
        return Err(rejected(
            "choose a new local tunnel config and loopback target",
        ));
    }
    let name = format!("{name}-local");
    let tunnel = crate::tunnel::create(bin, &name)
        .await
        .map_err(|_| rejected("local tunnel creation failed"))?;
    crate::tunnel::route_dns(bin, &name, hostname)
        .await
        .map_err(|_| rejected("local tunnel DNS setup failed"))?;
    write_config(
        config,
        &tunnel.id,
        &tunnel.credentials_file,
        hostname,
        local,
    )
}
pub fn spawn(bin: &Path, config: &Path) -> Result<Child> {
    Command::new(bin)
        .args(["--no-autoupdate", "tunnel", "--config"])
        .arg(config)
        .arg("run")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| rejected("cannot launch local tunnel"))
}
