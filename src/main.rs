//! GameModeExecutor: watch for a running game and run configured executables
//! when it starts and stops.

#[cfg(not(windows))]
compile_error!("GameModeExecutor only targets Windows");

mod actions;
mod config;
mod detect;
mod engine;
mod logging;
mod task;
mod win;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use config::Config;

/// Default config file shipped with the program, also used by `init`.
const EXAMPLE_CONFIG: &str = include_str!("../config.example.toml");

#[derive(Parser, Debug)]
#[command(name = "gamemode-executor", version, about, long_about = None)]
struct Cli {
    /// Path to the configuration file. Defaults to config.toml next to the
    /// executable, then %APPDATA%\GameModeExecutor\config.toml.
    #[arg(short, long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Override general.log_level.
    #[arg(long, global = true, value_name = "LEVEL")]
    log_level: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Watch for games and run the configured actions (default).
    Run {
        /// Hide the console window, for a logon-started instance.
        #[arg(long)]
        hidden: bool,
    },
    /// Print what the detectors currently see, then exit.
    Status,
    /// Run one set of actions immediately, without any detection.
    Trigger {
        #[arg(value_enum)]
        event: TriggerEvent,
    },
    /// Check that the configuration file is valid.
    Validate,
    /// Write a starter configuration file.
    Init {
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
    /// Register a per-user logon task that starts the watcher hidden.
    InstallTask {
        /// Delay after logon, as HHHH:MM.
        #[arg(long, default_value = "0000:15")]
        delay: String,
    },
    /// Remove the logon task.
    UninstallTask,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum TriggerEvent {
    Start,
    Stop,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Init { force }) => return cmd_init(cli.config.as_deref(), force),
        Some(Commands::InstallTask { delay }) => {
            let path = resolve_config_path(cli.config.clone())?;
            return task::install(&path, &delay);
        }
        Some(Commands::UninstallTask) => return task::uninstall(),
        _ => {}
    }

    let path = resolve_config_path(cli.config.clone())?;
    let config = Config::load(&path)?;
    let level = cli
        .log_level
        .unwrap_or_else(|| config.general.log_level.clone());

    match cli.command {
        Some(Commands::Validate) => {
            println!("Configuration `{}` is valid.", path.display());
            Ok(())
        }
        Some(Commands::Status) => {
            let _guards = logging::init(&level, None, 0, true)?;
            cmd_status(&config)
        }
        Some(Commands::Trigger { event }) => {
            let _guards = logging::init(&level, None, 0, true)?;
            let engine = engine::Engine::new(config);
            match event {
                TriggerEvent::Start => engine.fire_start_manual(),
                TriggerEvent::Stop => engine.fire_stop(None),
            }
            Ok(())
        }
        command => {
            let hidden = matches!(command, Some(Commands::Run { hidden: true }));
            cmd_run(config, &level, hidden)
        }
    }
}

fn cmd_run(config: Config, level: &str, hidden: bool) -> Result<()> {
    if hidden {
        win::hide_console();
    }
    // A hidden instance has nowhere to print, so give it a default log directory.
    let log_dir = config.general.log_dir.clone().or_else(|| {
        hidden
            .then(|| config::roaming_dir().map(|dir| dir.join("logs")))
            .flatten()
    });
    let _guards = logging::init(
        level,
        log_dir.as_deref(),
        config.general.log_keep_days,
        !hidden,
    )?;
    let _instance = win::SingleInstance::acquire("GameModeExecutor")?;

    let stop = Arc::new(AtomicBool::new(false));
    let handler_stop = Arc::clone(&stop);
    ctrlc::set_handler(move || handler_stop.store(true, Ordering::Relaxed))
        .context("cannot install the Ctrl-C handler")?;

    tracing::info!("GameModeExecutor {} starting", env!("CARGO_PKG_VERSION"));
    let mut engine = engine::Engine::new(config);
    engine.run(stop)?;
    tracing::info!("stopped");
    Ok(())
}

fn cmd_status(config: &Config) -> Result<()> {
    let detectors = detect::Detectors::new(&config.detection);
    let snapshot = detectors.snapshot()?;
    println!("Processes visible: {}", snapshot.processes.len());

    match detect::fullscreen::notification_state() {
        Ok(state) => println!(
            "Shell notification state: {} ({state})",
            detect::fullscreen::state_label(state)
        ),
        Err(error) => println!("Shell notification state: unavailable ({error})"),
    }

    if let Some(pid) = detect::fullscreen::foreground_pid() {
        let name = snapshot
            .by_pid(pid)
            .map(|process| process.name.clone())
            .unwrap_or_else(|| "?".to_owned());
        println!("Foreground process: {name} (pid {pid})");
    }

    match detectors.detect(&snapshot) {
        Some(signal) => println!("Detection: GAME -> {}", signal.describe()),
        None => println!("Detection: no game"),
    }
    Ok(())
}

fn cmd_init(explicit: Option<&std::path::Path>, force: bool) -> Result<()> {
    let path = match explicit {
        Some(path) => path.to_path_buf(),
        None => config::roaming_dir()
            .context("cannot determine %APPDATA%")?
            .join(config::CONFIG_FILE_NAME),
    };
    if path.exists() && !force {
        anyhow::bail!(
            "`{}` already exists (use --force to overwrite)",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create `{}`", parent.display()))?;
    }
    std::fs::write(&path, EXAMPLE_CONFIG)
        .with_context(|| format!("cannot write `{}`", path.display()))?;
    println!("Wrote {}", path.display());
    Ok(())
}

fn resolve_config_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    match explicit {
        Some(path) => Ok(path),
        None => config::default_path(),
    }
}
