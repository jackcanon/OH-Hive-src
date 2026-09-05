//! `hive` — OH Hive node CLI (ADR-003 D66).
//!
//! Subcommands map to what the desktop app does in its GUI (ADR-010), so a
//! headless Linux box can be a compute node without Tauri.

use anyhow::Result;
use clap::{Parser, Subcommand};
use ohhive_core::backend::{collect, mock::MockBackend, Backend};
use ohhive_core::capability::Requirements;
use ohhive_core::job::{Job, JobKind};

#[derive(Parser)]
#[command(name = "hive", version = ohhive_core::VERSION, about = "OH Hive node")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Probe hardware and print the capabilities this node would advertise.
    Probe,
    /// Run one prompt through a backend locally (no coordinator). Default backend: mock.
    Run {
        prompt: String,
        #[arg(long, default_value = "mock")]
        backend: String,
    },
    /// Register this node with the Hive (ADR-010 registration flow). Not implemented.
    Register,
    /// Check in / out of the Hive. Not implemented.
    CheckIn,
    CheckOut,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Probe => {
            let caps = MockBackend.capabilities().await?;
            println!("{}", serde_json::to_string_pretty(&caps)?);
        }
        Cmd::Run { prompt, backend } => {
            let be: Box<dyn Backend> = match backend.as_str() {
                "mock" => Box::new(MockBackend),
                other => anyhow::bail!("unknown backend '{other}' (available: mock)"),
            };
            let job = Job {
                id: uuid::Uuid::new_v4(),
                kind: JobKind::Inference,
                project_id: uuid::Uuid::nil(),
                card_id: None,
                parent: None,
                requirements: Requirements::default(),
                input: serde_json::json!({ "prompt": prompt }),
                resume_from: None,
                created_at: chrono::Utc::now(),
            };
            let stream = be.run(&job).await?;
            let (text, usage) = collect(stream).await?;
            println!("{text}");
            eprintln!("usage: {usage:?}");
        }
        Cmd::Register | Cmd::CheckIn | Cmd::CheckOut => {
            anyhow::bail!("not implemented — needs the coordinator client (ADR-004/005)")
        }
    }
    Ok(())
}
