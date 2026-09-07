//! `hive-server` CLI — a thin shell over [`hive_server::serve`] (the desktop app is the other shell).
//! Pairing: `hive pair` (choose "Regional server") writes the node.env this binary reads.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hive_server::{ServeOptions, ServerStatus};
use ohhive_core::hub::HubClient;
use std::{path::PathBuf, sync::Arc};

#[derive(Parser)]
#[command(name = "hive-server", version = ohhive_core::VERSION, about = "OH Hive regional server")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Register with the hub and serve. Ctrl-C / SIGTERM shuts down cleanly.
    Serve {
        /// Public base URL members and nodes can reach this server at (Cloudflare Tunnel hostname or public IP).
        #[arg(long, env = "HIVE_PUBLIC_URL")]
        public_url: String,
        /// Listen address.
        #[arg(long, env = "HIVE_LISTEN", default_value = "0.0.0.0:8790")]
        listen: String,
        /// Where blobs live. Default: ~/.local/share/ohhive/blobs
        #[arg(long, env = "HIVE_DATA_DIR")]
        data_dir: Option<PathBuf>,
        /// Disk offered, GB (reported to the hub; not enforced yet).
        #[arg(long, env = "HIVE_STORAGE_GB", default_value_t = 50)]
        storage_gb: u32,
        /// volunteer | hjm  (hjm only for HJM-run standby/anchor boxes, ADR-013 D76)
        #[arg(long, env = "HIVE_OPERATOR", default_value = "volunteer")]
        operator: String,
        /// primary | standby
        #[arg(long, env = "HIVE_TIER", default_value = "primary")]
        tier: String,
        #[arg(long, env = "HIVE_REGION")]
        region: Option<String>,
        /// Max upload size, MB.
        #[arg(long, env = "HIVE_MAX_UPLOAD_MB", default_value_t = 512)]
        max_upload_mb: usize,
        /// age public key (age1…) to encrypt nightly hub backups to. HJM-operated coordinators only. Unset = off.
        #[arg(long, env = "HIVE_BACKUP_RECIPIENT")]
        backup_recipient: Option<String>,
        /// UTC hour after which the daily backup runs (default 09 = 02:00 Phoenix).
        #[arg(long, env = "HIVE_BACKUP_HOUR_UTC", default_value_t = 9)]
        backup_hour_utc: u32,
    },
    /// Run one hub backup now (export → gzip → age → store → announce) and exit. Needs operator=hjm.
    Backup {
        #[arg(long, env = "HIVE_DATA_DIR")]
        data_dir: Option<PathBuf>,
        #[arg(long, env = "HIVE_BACKUP_RECIPIENT")]
        backup_recipient: String,
    },
    /// Print what the hub knows about this server's identity.
    Status,
    /// Run one garbage-collection pass now (drop blobs the hub says are unpinned past grace) and exit.
    Gc {
        #[arg(long, env = "HIVE_DATA_DIR")]
        data_dir: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Kept alive for the whole process: dropping it early would silently truncate the log file.
    let _log_guard = ohhive_core::logging::init("hive-server");
    ohhive_core::nodeconfig::export_env(); // node.env → env so HIVE_PUBLIC_URL etc. work via `hive set`
    let cli = Cli::parse();
    let cfg = ohhive_core::nodeconfig::load()?;
    let key = cfg
        .node_key
        .clone()
        .context("no node key — run `hive pair` and choose \"Regional server\"")?;
    let hub = Arc::new(HubClient::new(&cfg.hub_url, &cfg.anon_key, key));

    match cli.cmd {
        Cmd::Status => {
            let me = hub.whoami().await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "node_id": me.node_id, "display_name": me.display_name, "role": me.role, "region": me.region, "presence": me.presence,
                    "version": ohhive_core::VERSION,
                }))?
            );
        }
        Cmd::Gc { data_dir } => {
            let (dropped, left) = hive_server::gc_once(&hub, data_dir).await?;
            println!(
                "{}",
                serde_json::json!({ "dropped": dropped, "blobs_left": left })
            );
        }
        Cmd::Backup {
            data_dir,
            backup_recipient,
        } => {
            let (hash, bytes, plain) =
                hive_server::backup_once(&hub, data_dir, &backup_recipient).await?;
            println!(
                "{}",
                serde_json::json!({ "hash": hash, "bytes": bytes, "plaintext_bytes": plain, "path": format!("/a/{hash}") })
            );
        }
        Cmd::Serve {
            public_url,
            listen,
            data_dir,
            storage_gb,
            operator,
            tier,
            region,
            max_upload_mb,
            backup_recipient,
            backup_hour_utc,
        } => {
            let opts = ServeOptions {
                public_url,
                listen,
                data_dir,
                storage_gb,
                operator,
                tier,
                region,
                max_upload_mb,
                backup_recipient,
                backup_hour_utc,
            };
            hive_server::serve(
                &cfg,
                hub,
                opts,
                shutdown_signal(),
                Arc::new(ServerStatus::default()),
            )
            .await?;
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("sigterm");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
