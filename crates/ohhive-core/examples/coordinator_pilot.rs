//! Bounded real-worker pilot. Explicit credentials and endpoint; never changes node.env/defaults.
use hive_core::{
    backend::{llama_cpp::LlamaCppBackend, Backend},
    capability::ToolsLevel,
    coordinator_hub::CoordinatorHub,
    hub::Hub,
    worker::Worker,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(a.len()==5,"usage: coordinator_pilot <origin> <node-key-file> <model-origin> <scratch-dir> <max-cards>");
    let key = std::fs::read_to_string(&a[1])?;
    let key = key.trim();
    let hub = std::sync::Arc::new(CoordinatorHub::connect(&a[0], key, None).await?);
    let backend = LlamaCppBackend::new(&a[2]);
    let mut caps = backend.capabilities().await?;
    caps.tools_level = ToolsLevel::InferenceOnly;
    caps.allow_internet = false;
    hub.check_in(&caps, None).await?;
    let (stop, rx) = tokio::sync::watch::channel(false);
    let stop_signal = stop.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = stop_signal.send(true);
    });
    let h = hub.clone();
    let hb = tokio::spawn(async move {
        let mut last = None;
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            match h.heartbeat(last).await {
                Ok((_, ms)) => {
                    last = Some(ms);
                    println!("heartbeat_ms={ms}");
                }
                Err(_) => {
                    eprintln!("pilot heartbeat failed; stopping worker");
                    let _ = stop.send(true);
                    break;
                }
            }
        }
    });
    let worker = Worker {
        hub: hub.as_ref(),
        backend: &backend,
        caps: &caps,
        default_model: caps.models.first().map(|m| m.id.clone()),
        stop: rx,
        events: None,
        data_dir: PathBuf::from(&a[3]),
        sandbox: None,
    };
    let result = async {
        for n in 0..a[4].parse::<usize>()? {
            let at = Instant::now();
            if !worker.tick().await? {
                println!("pilot_idle_after_cards={n}");
                break;
            }
            println!(
                "pilot_card={} elapsed_seconds={:.3}",
                n + 1,
                at.elapsed().as_secs_f64()
            );
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    hb.abort();
    let _ = hub.check_out().await;
    result
}
