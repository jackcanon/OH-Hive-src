//! `hive-server` — regional server (ADR-004).
//!
//! Responsibilities (all TODO, tracked as Cmd Work items):
//! - libp2p bootstrap + relay v2 for nodes behind NAT (D28)
//! - content-addressed artifact store with replication factor 2 (ADR-007)
//! - model-weight cache so nodes pull GGUFs from the nearest server (ADR-004)
//! - coordinator candidate: try to take `hive.coordinator_lease` (ADR-005)
//!
//! Footprint rule: this binary must stay a single static executable with no
//! Python and no GPU deps. If a dependency would break the Pi build, it goes
//! in `hive` or the desktop app instead.

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "hive-server", version = ohhive_core::VERSION, about = "OH Hive regional server")]
struct Cli {
    /// Region label, e.g. "us-west". Derived from IP at registration if omitted (ADR-004 D58).
    #[arg(long, env = "HIVE_REGION")]
    region: Option<String>,
    /// Disk offered for artifacts and model cache, in GB.
    #[arg(long, env = "HIVE_STORAGE_GB", default_value_t = 50)]
    storage_gb: u32,
    /// Listen address for the overlay.
    #[arg(long, env = "HIVE_LISTEN", default_value = "0.0.0.0:4001")]
    listen: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    let cli = Cli::parse();
    tracing::info!(
        version = ohhive_core::VERSION,
        region = ?cli.region,
        storage_gb = cli.storage_gb,
        listen = %cli.listen,
        "hive-server starting (skeleton — overlay/store/coordinator not implemented)"
    );
    tokio::signal::ctrl_c().await?;
    tracing::info!("hive-server shutting down");
    Ok(())
}
