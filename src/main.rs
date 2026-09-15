//! GameModeExecutor: watch for a running game and run configured executables
//! when it starts and stops.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use game_mode_executor::config::{self, Config};
use game_mode_executor::detect::known_games::KnownGames;
use game_mode_executor::detect::presence_writer;
use game_mode_executor::detect::process::Snapshot;
use game_mode_executor::{build_info, detect, engine, exit, logging, marker, service, task};

/// Default config file shipped with the program, also used by `init`.
const EXAMPLE_CONFIG: &str = include_str!("../config.example.toml");

#[derive(Parser, Debug)]
#[command(
    name = "gamemode-executor",
    version = build_info::VERSION,
    long_version = build_info::LONG_VERSION,
    about,
    long_about = None
)]
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
    ///
    /// For an unattended instance use `gamemode-executorw.exe`, which is the
    /// same watcher with no console at all. `install-task` registers that one.
    Run {
        /// Accepted and ignored. A task registered before this binary had a
        /// windowless twin still passes it, and refusing it would stop that
        /// task dead at the next logon.
        #[arg(long, hide = true)]
        hidden: bool,
    },
    /// Print what the detectors currently see, then exit.
    Status,
    /// Run one set of actions immediately, without any detection.
    Trigger {
        #[arg(value_enum)]
        event: TriggerEvent,
    },
    /// Ask whether Windows knows a given executable as a game.
    Check {
        /// Full path of an executable. Omit when using --pid.
        path: Option<String>,
        /// Inspect a running process instead, by process id. Shows what the
        /// naming code actually reads, which is the only way to tell a missing
        /// entry from an unreadable process.
        #[arg(long, conflicts_with = "path")]
        pid: Option<u32>,
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
        /// Delay after logon, e.g. `15s` or `1m`.
        #[arg(long, default_value = "15s")]
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

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            std::process::ExitCode::from(exit::code_for(&error))
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Init { force }) => return cmd_init(cli.config.as_deref(), force),
        Some(Commands::InstallTask { delay }) => {
            let delay = humantime::parse_duration(&delay)
                .with_context(|| format!("cannot read `{delay}` as a delay"))?;
            let path = resolve_config_path(cli.config.clone())?;
            return task::install(&path, delay);
        }
        Some(Commands::UninstallTask) => return task::uninstall(),
        Some(Commands::Check { path, pid }) => return cmd_check(path.as_deref(), pid),
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
            let _guards = logging::init(&level, None, true)?;
            cmd_status(&config)
        }
        Some(Commands::Trigger { event }) => {
            let _guards = logging::init(&level, None, true)?;
            let engine = engine::Engine::new(config)?;
            match event {
                TriggerEvent::Start => engine.fire_start_manual(),
                TriggerEvent::Stop => {
                    engine.fire_stop(None);
                }
            }
            Ok(())
        }
        _ => cmd_run(config, &path, &level),
    }
}

fn cmd_run(config: Config, config_path: &std::path::Path, level: &str) -> Result<()> {
    // This binary is a console program, so it always has one to log to.
    service::serve(config, config_path, level, true)
}

fn print_pending(pending: &marker::Pending, marker: &marker::Marker) {
    println!(
        "  game               : {}",
        pending.game.as_deref().unwrap_or("not named")
    );
    println!(
        "  since              : {}",
        pending.since.as_deref().unwrap_or("unknown")
    );
    println!("  file               : {}", marker.path().display());
}

fn cmd_status(_config: &Config) -> Result<()> {
    // First, because when someone is diagnosing a machine that is not theirs,
    // knowing which build they are looking at comes before anything it reports.
    println!("Build                : {}", build_info::VERSION);
    println!("  commit             : {}", build_info::COMMIT_DISPLAY);
    println!("  documentation      : {}", build_info::DOCS_URL);

    let snapshot = Snapshot::take()?;

    // The detector itself.
    let mut game_running = false;
    match presence_writer::registered_exe() {
        Ok(exe) => {
            println!("Presence writer      : {}", exe.display());
            println!(
                "  Microsoft default  : {}",
                if presence_writer::is_microsoft_default(&exe) {
                    "yes"
                } else {
                    "NO - something else owns the registration"
                }
            );
            match presence_writer::running_pid(&exe) {
                Some(pid) => {
                    game_running = true;
                    println!("  running            : YES (pid {pid}) - a game is running");
                }
                None => println!("  running            : no - no game running"),
            }
        }
        Err(error) => println!("Presence writer      : unavailable ({error:#})"),
    }

    // The marker means one of two things, and the writer tells them apart: a
    // session open right now, which is the watcher doing its job, or one that
    // never closed, which the watcher settles at its next start. Read with a
    // game on, the first wording used to claim the second, and was wrong.
    match marker::Marker::in_local_dir() {
        Some(marker) => match (marker.pending(), game_running) {
            (Some(pending), true) => {
                println!("Session marker       : present - a game session is open, as expected");
                print_pending(&pending, &marker);
            }
            (Some(pending), false) => {
                println!(
                    "Session marker       : PRESENT with no game running - the last session \
                     never closed; the stop commands run when the watcher next starts"
                );
                print_pending(&pending, &marker);
            }
            (None, true) => println!(
                "Session marker       : NONE while a game is running - the watcher has not \
                 recorded this session; is it running? ({})",
                marker.path().display()
            ),
            (None, false) => {
                println!("Session marker       : none ({})", marker.path().display())
            }
        },
        None => println!("Session marker       : unavailable, no local profile"),
    }

    // Naming only, never detection.
    let known = KnownGames::load();
    match &known {
        Ok(known) => {
            let counts = known.counts();
            println!(r"Known Game List (HKCU\System\GameConfigStore\Children)");
            println!("  entries seen         : {}", counts.entries);
            println!("  executable paths     : {}", counts.exe_paths);
            println!(
                "  parent directories   : {} paths, {} names",
                counts.parent_paths, counts.parent_names
            );
            println!("  packaged titles      : {}", counts.packages);
            if !known.skipped_generic.is_empty() {
                println!(
                    "  names too generic    : {}",
                    known.skipped_generic.join(", ")
                );
            }
            // Every match, with what the GPU says about it. A title brings
            // several: seeing them ranked is the only way to tell whether the
            // right one would be picked.
            let candidates = known.candidates(&snapshot);
            if candidates.is_empty() {
                println!("  matching processes   : none");
            } else {
                let load = detect::gpu::rendering_load(std::time::Duration::from_millis(500))
                    .unwrap_or_default();
                println!("  matching processes   : {}", candidates.len());
                for candidate in &candidates {
                    let share = candidate
                        .process_id
                        .and_then(|pid| load.get(&pid))
                        .copied()
                        .unwrap_or(0.0);
                    println!("    {share:>6.1}% rendering  {}", candidate.describe());
                }
                match detect::most_active(candidates, &load) {
                    Some(best) => println!("  would be named       : {}", best.describe()),
                    None => println!("  would be named       : none"),
                }
            }
        }
        Err(error) => println!("Known Game List      : unavailable ({error:#})"),
    }

    print_foreground(&snapshot, known.as_ref().ok());

    println!("Processes visible    : {}", snapshot.processes.len());
    match detect::fullscreen::notification_state() {
        Ok(state) => println!(
            "Shell notification   : {} ({}) [diagnostic only]",
            detect::fullscreen::state_label(state),
            state.0
        ),
        Err(error) => println!("Shell notification   : unavailable ({error})"),
    }
    Ok(())
}

fn cmd_check(path: Option<&str>, pid: Option<u32>) -> Result<()> {
    let known = detect::known_games::KnownGames::load()?;

    if let Some(pid) = pid {
        return check_pid(&known, pid);
    }

    let path = path.expect("clap requires a path when --pid is absent");
    match known.match_exe(path) {
        Some(kind) => println!("{path}\n  -> game, matched by {}", kind.label()),
        None => println!("{path}\n  -> not a known game"),
    }
    Ok(())
}

/// Show what the naming code actually reads for one process. Without this,
/// a process that cannot be opened is indistinguishable from one Windows
/// simply does not list as a game.
fn check_pid(known: &KnownGames, pid: u32) -> Result<()> {
    let snapshot = Snapshot::take()?;
    let name = snapshot
        .by_pid(pid)
        .map(|process| process.name.clone())
        .unwrap_or_else(|| "(not running)".to_owned());
    println!("pid {pid}: {name}");

    let identity = detect::process::identity(pid);
    let image = identity.path;
    match &image {
        Some(image) => println!("  image path     : {image}"),
        None => println!("  image path     : UNREADABLE (cannot open the process)"),
    }

    let family = identity.package_family;
    match &family {
        Some(family) => println!("  package family : {family}"),
        None => println!("  package family : none (not packaged, or unreadable)"),
    }

    let verdict = image
        .as_deref()
        .and_then(|image| known.match_exe(image))
        .map(|kind| kind.label().to_owned())
        .or_else(|| {
            family
                .as_deref()
                .filter(|family| known.match_package(family))
                .map(|_| "package family".to_owned())
        });
    match verdict {
        Some(kind) => println!("  -> game, matched by {kind}"),
        None => println!("  -> not a known game"),
    }
    Ok(())
}

/// The foreground process is what Game Mode itself applies to, so it is the
/// interesting one to check against the Known Game List.
fn print_foreground(
    snapshot: &detect::process::Snapshot,
    known: Option<&detect::known_games::KnownGames>,
) {
    let Some(pid) = detect::fullscreen::foreground_pid() else {
        println!("Foreground           : none");
        return;
    };
    let name = snapshot
        .by_pid(pid)
        .map(|process| process.name.clone())
        .unwrap_or_else(|| "?".to_owned());
    let path = detect::process::full_path(pid);
    println!("Foreground           : {name} (pid {pid})");
    match &path {
        Some(path) => println!("  path               : {path}"),
        None => println!("  path               : (not readable)"),
    }
    let verdict = match (known, &path) {
        (Some(known), Some(path)) => match known.match_exe(path) {
            Some(kind) => format!("yes, via {}", kind.label()),
            None => "no".to_owned(),
        },
        _ => "unknown".to_owned(),
    };
    println!("  Windows calls it a game: {verdict}");
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
