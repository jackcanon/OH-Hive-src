//! Shared stdout + rolling-file logging (ops-driven, 2026-09-07: a live Edge Function bug took
//! several minutes to diagnose purely because the code swallowed the real error; the audit that
//! followed found the same gap here, worse -- `hive-server`, `hive`, and the desktop app each
//! called `tracing_subscriber::fmt()` straight to stdout, with nothing durable behind it. For
//! `hive-server` that only works if a service manager happens to be capturing stdout; for the
//! desktop app, launched with no attached terminal, an error on a member's machine left *nothing*
//! to look at at all.
//!
//! This gives every long-running binary a daily-rolling log file at
//! `~/.config/ohhive/logs/<app>.log.<date>` alongside the existing stdout output, so there is
//! always something to open after the fact. `hive-coordinator`'s scheduler logic and the shared
//! worker loop in [`crate::worker`] don't call this themselves -- they just emit `tracing` events,
//! which are captured by whichever binary's subscriber is active (installed once, at startup).

use std::path::PathBuf;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// `~/.config/ohhive/logs` — sibling of `nodeconfig::path()`'s `~/.config/ohhive/node.env`.
pub fn log_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ohhive")
        .join("logs")
}

/// Install a combined stdout + rolling-file subscriber. Call once, as early as possible in
/// `main()`/`run()`. `app` names the log file (`hive-server`, `hive`, `desktop`, ...).
///
/// Returns a guard that must be kept alive for the process's entire lifetime — the file writer is
/// non-blocking and flushes on a background thread; dropping the guard early silently truncates
/// buffered log lines. Bind it (`let _log_guard = hive_core::logging::init("hive-server");`) in
/// the same scope that runs for the life of the process, never in a temporary.
pub fn init(app: &str) -> tracing_appender::non_blocking::WorkerGuard {
    let dir = log_dir();
    // Best-effort: if the directory can't be created (read-only home, odd permissions), logging
    // falls back to stdout-only rather than failing the whole process over a log file.
    let _ = std::fs::create_dir_all(&dir);
    let file_appender = tracing_appender::rolling::daily(&dir, format!("{app}.log"));
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter())
        .with(fmt::layer())
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();

    guard
}

/// Native Swift hosts do not install the CLI subscriber. Capture model identity metadata and
/// Bots runner warnings (why a turn was aborted) here, once per process, without enabling general
/// request/body logging. Added 2026-09-24 after a silently failing agent could not be diagnosed.
pub fn init_model_identity() {
    static GUARD: std::sync::OnceLock<Option<tracing_appender::non_blocking::WorkerGuard>> =
        std::sync::OnceLock::new();
    GUARD.get_or_init(|| {
        let dir = log_dir();
        std::fs::create_dir_all(&dir).ok()?;
        let appender = tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("model-identity.log")
            .max_log_files(7)
            .build(dir)
            .ok()?;
        let (writer, guard) = tracing_appender::non_blocking(appender);
        tracing_subscriber::registry()
            .with(EnvFilter::new(
                "off,hive_model_identity=info,hive_core::bots=warn",
            ))
            .with(fmt::layer().with_writer(writer).with_ansi(false))
            .try_init()
            .ok()?;
        Some(guard)
    });
}
