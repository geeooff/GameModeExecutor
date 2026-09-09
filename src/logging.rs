//! Console and file logging setup.

use std::path::Path;

use anyhow::{Context, Result};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};
use windows::Win32::System::SystemInformation::GetLocalTime;

/// Timestamps in the reader's own time zone.
///
/// The default is UTC, which files an event that happened at 00:46 under the
/// previous day at 22:46. For a log whose only purpose is to be read by the
/// person who just played a game, that is a defect. `GetLocalTime` avoids both
/// the `time` crate's local-offset caveats and an extra feature flag, and
/// matches the format `presence-probe` already writes.
struct LocalTimestamp;

impl FormatTime for LocalTimestamp {
    fn format_time(&self, writer: &mut Writer<'_>) -> std::fmt::Result {
        let now = unsafe { GetLocalTime() };
        write!(
            writer,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond, now.wMilliseconds
        )
    }
}

/// Keeps the background writer of the file appender alive.
pub struct Guards(#[allow(dead_code)] Vec<WorkerGuard>);

/// Initialise logging. `RUST_LOG` overrides `level` when set.
pub fn init(level: &str, log_dir: Option<&Path>, to_console: bool) -> Result<Guards> {
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
            // One file, not a daily rotation. This log gains a handful of lines
            // per game session, and rotation only bought filenames dated in UTC
            // -- the very confusion the local timestamps above remove.
            let appender = tracing_appender::rolling::never(dir, "gamemode-executor.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            guards.push(guard);
            Some(
                fmt::layer()
                    .with_ansi(false)
                    .with_timer(LocalTimestamp)
                    .with_writer(writer),
            )
        }
        None => None,
    };

    let console_layer =
        to_console.then(|| fmt::layer().with_target(false).with_timer(LocalTimestamp));

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    Ok(Guards(guards))
}
