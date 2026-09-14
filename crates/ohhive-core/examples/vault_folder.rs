//! Developer host for a selected local Markdown folder. No account/cloud configuration.
use hive_core::local_hub::{serve, LocalHubStore};
use std::time::Duration;
fn arg(a: &[String], n: usize) -> anyhow::Result<&str> {
    a.get(n)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing argument"))
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("attach") => {
            let s = LocalHubStore::open(arg(&a,1)?)?;
            let vault = s.vault_create("Local notes")?;
            s.vault_attach_folder(vault,arg(&a,2)?)?;
            // An existing paired/enrolled node must be explicitly selected as reader.
            s.vault_grant(vault,arg(&a,3)?.parse()?,true)?;
            println!("Vault {vault}. Start the host to scan it.");
        }
        Some("serve") => {
            let s = LocalHubStore::open(arg(&a,1)?)?;
            let vault = arg(&a,2)?.parse()?;
            s.vault_scan_status(vault)?;
            let listener = tokio::net::TcpListener::bind(arg(&a,3)?).await?;
            let (stop,rx)=tokio::sync::watch::channel(false);
            let store=s.clone();
            let watcher=tokio::spawn(async move{store.vault_watch_folder(vault,Duration::from_secs(5),rx).await});
            println!("Vault host listening on {}",listener.local_addr()?);
            let result=serve(s,listener,async {let _=tokio::signal::ctrl_c().await;}).await;
            let _=stop.send(true);
            watcher.await??;
            result?;
        }
        _ => println!("Local-folder developer commands:\n  attach <hub-db> <folder> <existing-reader-node-id>\n  serve <hub-db> <vault-id> <private-bind-address:port>\nUse one store/host process per database. Keep the database outside the note folder. Polls every five seconds; stops with Ctrl-C."),
    }
    Ok(())
}
