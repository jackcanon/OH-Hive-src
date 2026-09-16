//! No real token, no login, no model turn. Exercises the pinned runtime's handshake only.
use github_copilot_sdk::Client;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::temp_dir().join(format!("hive-copilot-probe-{}", std::process::id()));
    std::fs::create_dir(&home)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700))?;
    }
    let options =
        hive_copilot_conformance::options("ghu_invalid_conformance_fixture".into(), &home)?;
    let client = tokio::time::timeout(Duration::from_secs(30), Client::start(options)).await??;
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        client.ping(Some("hive-conformance")),
    )
    .await;
    let stopped = tokio::time::timeout(Duration::from_secs(10), client.stop()).await;
    result??;
    stopped??;
    std::fs::remove_dir_all(home)?;
    println!("PASS: official SDK/runtime started, pinged and stopped; no account or model test performed.");
    Ok(())
}
