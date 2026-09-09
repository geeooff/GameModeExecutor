//! Console and rolling-file logging setup.

use std::path::Path;

use anyhow::{Context, Result};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::Rotation;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

/// Keeps the background writer of the file appender alive.
pub struct Guards(#[allow(dead_code)] Vec<WorkerGuard>);

/// Initialise logging. `RUST_LOG` overrides `level` when set.
pub fn init(
    level: &str,
    log_dir: Option<&Path>,
    keep_days: usize,
    to_console: bool,
) -> Result<Guards> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "game_mode_executor={level},gamemode_executor={level}"
        ))
    });

    let mut guards = Vec::new();
    let file_layer = match log_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create log directory `{}`", dir.display()))?;
            let appender = tracing_appender::rolling::Builder::new()
                .rotation(Rotation::DAILY)
                .filename_prefix("gamemode-executor")
                .filename_suffix("log")
                .max_log_files(keep_days.max(1))
                .build(dir)
                .with_context(|| format!("cannot open log file in `{}`", dir.display()))?;
            let (writer, guard) = tracing_appender::non_blocking(appender);
            guards.push(guard);
            Some(fmt::layer().with_ansi(false).with_writer(writer))
        }
        None => None,
    };

    let console_layer = to_console.then(|| fmt::layer().with_target(false));

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    Ok(Guards(guards))
}
