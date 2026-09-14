//! Separate opt-in pilot process; never starts coordinator election or modifies normal routing.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _guard = hive_core::logging::init("hive-control-pilot");
    let db = std::env::var("HIVE_CTL_DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("HIVE_CTL_DATABASE_URL required"))?;
    let listen = std::env::var("HIVE_CTL_LISTEN").unwrap_or_else(|_| "127.0.0.1:8791".into());
    let address: std::net::SocketAddr = listen.parse()?;
    anyhow::ensure!(
        address.ip().is_loopback(),
        "pilot must listen on loopback behind HTTPS"
    );
    hive_server::control::Control::connect(&db)
        .await?
        .serve(&listen, async {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("signal");
                tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
        })
        .await
}
