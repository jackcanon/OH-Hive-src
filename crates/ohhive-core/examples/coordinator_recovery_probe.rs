//! Bounded auth/recovery probe; no claims, outputs, or funding. Config is a private JSON file.
use hive_core::{
    coordinator_hub::{AuthorityDelegation, CoordinatorHub},
    hub::Hub,
};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(std::env::args().nth(1).expect("private test directory"));
    let c: Value = serde_json::from_slice(&std::fs::read(dir.join("config.json"))?)?;
    let source = AuthorityDelegation::new(
        c["authority"].as_str().unwrap(),
        c["anon_key"].as_str().unwrap().into(),
        c["node_key"].as_str().unwrap().into(),
        c["server_id"].as_str().unwrap().parse()?,
        c["project_id"].as_str().unwrap().parse()?,
    )?;
    let at = Instant::now();
    let hub =
        CoordinatorHub::connect_with_source(c["origin"].as_str().unwrap(), Arc::new(source), None)
            .await?;
    let leases = hub.recover_leases().await?;
    anyhow::ensure!(leases.is_empty(), "unexpected existing leases");
    hub.heartbeat(None).await?;
    println!("initial_auth_recovery_ms={}", at.elapsed().as_millis());
    for phase in ["database", "restart"] {
        std::fs::write(dir.join(format!("{phase}.waiting")), b"ready")?;
        let deadline = Instant::now() + Duration::from_secs(180);
        while !dir.join(format!("{phase}.go")).exists() {
            anyhow::ensure!(Instant::now() < deadline, "test orchestration timeout");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let at = Instant::now();
        hub.heartbeat(None).await?;
        anyhow::ensure!(
            hub.recover_leases().await?.is_empty(),
            "unexpected recovered lease"
        );
        println!(
            "{phase}_heartbeat_and_recovery_ms={}",
            at.elapsed().as_millis()
        );
    }
    println!("real CoordinatorHub authority issuance, reconnect, reauthentication and empty-lease recovery PASS");
    Ok(())
}
