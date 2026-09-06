//! `hive work` uses the shared worker in `ohhive_core::worker`; this shim only owns the
//! process-signal → stop-flag translation (Ctrl-C / SIGTERM from systemd or launchd).

pub use ohhive_core::worker::Worker;
use tokio::sync::watch;

/// A stop flag that flips on Ctrl-C or SIGTERM.
pub fn stop_on_signal() -> watch::Receiver<bool> {
    let (tx, rx) = watch::channel(false);
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = tx.send(true);
        // keep the sender alive until the process exits so the receiver keeps reading `true`
        std::future::pending::<()>().await;
    });
    rx
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("sigterm handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
