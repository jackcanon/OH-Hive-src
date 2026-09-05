//! `hive` — OH Hive node CLI (ADR-003 D66).
//!
//! Subcommands map to what the desktop app does in its GUI (ADR-010), so a
//! headless Linux box can be a compute node without Tauri.

use anyhow::Result;
use clap::{Parser, Subcommand};
use ohhive_core::backend::{mock::MockBackend, Backend};
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
        /// `mock`, or `llama_cpp` (requires the llama-cpp feature).
        #[arg(long, default_value = "mock")]
        backend: String,
        /// Base URL for llama_cpp: llama-server (http://127.0.0.1:8080) or Ollama (http://127.0.0.1:11434).
        #[arg(long, env = "HIVE_LLAMA_URL", default_value = "http://127.0.0.1:11434")]
        url: String,
        /// Model id as the server names it, e.g. `gemma3:4b` or `qwen3.6:latest`.
        #[arg(long, env = "HIVE_MODEL")]
        model: Option<String>,
        #[arg(long, default_value_t = 256)]
        max_tokens: u64,
    },
    /// List models the llama_cpp backend can see at --url.
    Models {
        #[arg(long, env = "HIVE_LLAMA_URL", default_value = "http://127.0.0.1:11434")]
        url: String,
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
        Cmd::Run { prompt, backend, url, model, max_tokens } => {
            let be: Box<dyn Backend> = match backend.as_str() {
                "mock" => Box::new(MockBackend),
                #[cfg(feature = "llama-cpp")]
                "llama_cpp" => Box::new(ohhive_core::backend::llama_cpp::LlamaCppBackend::new(url)),
                other => anyhow::bail!("unknown backend '{other}' (available: mock, llama_cpp[feature])"),
            };
            let job = Job {
                id: uuid::Uuid::new_v4(),
                kind: JobKind::Inference,
                project_id: uuid::Uuid::nil(),
                card_id: None,
                parent: None,
                requirements: Requirements { model_id: model, ..Default::default() },
                input: serde_json::json!({ "prompt": prompt, "max_tokens": max_tokens }),
                resume_from: None,
                created_at: chrono::Utc::now(),
            };
            let started = std::time::Instant::now();
            let mut stream = be.run(&job).await?;
            use futures::StreamExt;
            use std::io::Write;
            let mut usage = None;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                print!("{}", chunk.text);
                std::io::stdout().flush().ok();
                if let Some(u) = chunk.usage {
                    usage = Some(u);
                }
                if chunk.done {
                    break;
                }
            }
            println!();
            let u = usage.unwrap_or_default();
            let tps = if u.compute_seconds > 0.0 { u.tokens_out as f64 / u.compute_seconds } else { 0.0 };
            eprintln!(
                "backend={} tokens_in={} tokens_out={} compute={:.2}s ({:.1} tok/s) wall={:.2}s",
                be.name(),
                u.tokens_in,
                u.tokens_out,
                u.compute_seconds,
                tps,
                started.elapsed().as_secs_f64()
            );
        }
        Cmd::Models { url } => {
            #[cfg(feature = "llama-cpp")]
            {
                let be = ohhive_core::backend::llama_cpp::LlamaCppBackend::new(url);
                let caps = be.capabilities().await?;
                for m in caps.models {
                    println!("{}", m.id);
                }
            }
            #[cfg(not(feature = "llama-cpp"))]
            {
                let _ = url;
                anyhow::bail!("build with --features llama-cpp");
            }
        }
        Cmd::Register | Cmd::CheckIn | Cmd::CheckOut => {
            anyhow::bail!("not implemented — needs the coordinator client (ADR-004/005)")
        }
    }
    Ok(())
}
