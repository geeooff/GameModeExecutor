#![windows_subsystem = "windows"]
//! The watcher with no console, for the logon task.
//!
//! The twin of `gamemode-executor.exe`, running the same watcher from the same
//! library, differing only in subsystem. The two subsystems cannot live in one
//! file, which is why Python ships `python.exe` and `pythonw.exe`, and why this
//! exists rather than a flag.
//!
//! The split keeps a promise. A shell does not wait for a Windows-subsystem
//! process, so had the whole program moved here, `validate`'s exit code would
//! have stopped reaching scripts -- silently, which is the worst way for a
//! contract to break. Every command other than watching therefore stays in the
//! console binary, where a shell still waits, pipes still work and exit codes
//! still arrive.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use game_mode_executor::config::{self, Config};
use game_mode_executor::{exit, service};

#[derive(Parser, Debug)]
#[command(
    name = "gamemode-executorw",
    version = game_mode_executor::build_info::VERSION,
    long_version = game_mode_executor::build_info::LONG_VERSION,
    about = "Watches for games with no console. Use gamemode-executor.exe for every other command."
)]
struct Cli {
    /// Path to the configuration file. Defaults to config.toml next to the
    /// executable, then %APPDATA%\GameModeExecutor\config.toml.
    #[arg(short, long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Override general.log_level.
    #[arg(long, value_name = "LEVEL")]
    log_level: Option<String>,

    /// Accepted and ignored, so a task registered against the console binary
    /// keeps working if it is repointed here by hand.
    #[arg(long, hide = true)]
    hidden: bool,
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            // There is no console to print to. The log file has the detail, and
            // the exit code is what Task Scheduler records and shows.
            tracing::error!(
                target: game_mode_executor::logging::target::WATCHER,
                error = %format!("{error:#}"),
                "GameModeExecutor could not start"
            );
            std::process::ExitCode::from(exit::code_for(&error))
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let path = match cli.config {
        Some(path) => path,
        None => config::default_path()?,
    };
    let config = Config::load(&path)?;
    let level = cli
        .log_level
        .unwrap_or_else(|| config.general.log_level.clone());
    service::serve(config, &level, false)
}
