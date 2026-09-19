//! The command line, shared by both executables.
//!
//! `gamemode-executor.exe` and `gamemode-executorw.exe` parse the same
//! arguments and run the same commands; they differ in subsystem alone. The
//! console one is the one people type, because a shell waits for it and its
//! output and exit code arrive where a person or a script can see them. The
//! windowless one is what the logon task and the installer run: it prints
//! nothing, there is nowhere to print, and its exit code is the verdict.
//! Windows Installer starts an executable action without hiding its
//! console, so the console binary would flash a window twice at the end of
//! every install -- seen on 2026-09-18 -- which is why the installer runs
//! `init` and `install-task` through the twin.
//!
//! `init`, `install-task`, `uninstall-task` and `stop` write what they did to
//! the log, under `setup`, at `info`: the log then says who did what to this
//! machine and when, whether a person typed it or the installer ran it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use crate::config::{self, Config};
use crate::detect::known_games::KnownGames;
use crate::detect::presence_writer;
use crate::detect::process::Snapshot;
use crate::{actions, build_info, detect, logging, marker, purge, service, task};

#[derive(Parser, Debug)]
#[command(
    name = "gamemode-executor",
    version = build_info::VERSION,
    long_version = build_info::LONG_VERSION,
    about,
    long_about = None
)]
pub struct Cli {
    /// Path to the configuration file. Defaults to config.toml next to the
    /// executable, then %APPDATA%\GameModeExecutor\config.toml.
    #[arg(short, long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Override general.log_level.
    #[arg(long, global = true, value_name = "LEVEL")]
    pub log_level: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
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
    /// Write the starter configuration file. One already there is kept.
    Init {
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
    /// Register a per-user logon task that starts the watcher hidden, and
    /// start it now. A task already registered is kept.
    InstallTask {
        /// Delay after logon, e.g. `15s` or `1m`.
        #[arg(long, default_value = "15s")]
        delay: String,
        /// Replace a task that is already registered.
        #[arg(long)]
        force: bool,
    },
    /// Remove the logon task.
    UninstallTask,
    /// Stop the running watcher, the way Quit in its menu does: mid-game,
    /// the stop commands run on the way out. None running is not an error.
    /// The logon task is left as it is; `install-task` starts it again.
    Stop {
        /// Leave an open game session to the next watcher instead of
        /// closing it: the stop commands do not run, and the watcher that
        /// starts next resumes the session. For an update or an upgrade,
        /// where one follows within seconds.
        #[arg(long)]
        handover: bool,
    },
    /// Look for a newer release on GitHub and install it: downloaded,
    /// verified against the release's checksums, then run the way the
    /// installer would -- a running watcher hands its game session to the
    /// new one. The one command that connects to anything.
    Update {
        /// Only say whether a newer release exists.
        #[arg(long)]
        check: bool,
    },
    /// Remove every trace of the program: the logon task, the configuration,
    /// the log, the session marker, and the executables themselves. Refuses
    /// while a game is running. Shows what it will remove and asks first.
    Purge {
        /// Do not ask; for scripts.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum TriggerEvent {
    Start,
    Stop,
}

/// Run what the command line asks. `console` says whether this process has
/// one to write to; the log file is written either way.
pub fn run(cli: Cli, console: bool) -> Result<()> {
    match cli.command {
        Some(Command::Init { force }) => {
            setup_logging(cli.config.as_deref(), cli.log_level.as_deref(), console)?;
            let path = match cli.config {
                Some(path) => path,
                None => config::starter_path()?,
            };
            config::write_starter(&path, force)?;
            return Ok(());
        }
        Some(Command::InstallTask { delay, force }) => {
            setup_logging(cli.config.as_deref(), cli.log_level.as_deref(), console)?;
            let delay = humantime::parse_duration(&delay)
                .with_context(|| format!("cannot read `{delay}` as a delay"))?;
            let path = resolve_config_path(cli.config)?;
            task::install(&path, delay, force)?;
            return Ok(());
        }
        Some(Command::UninstallTask) => {
            setup_logging(cli.config.as_deref(), cli.log_level.as_deref(), console)?;
            return task::uninstall();
        }
        Some(Command::Stop { handover }) => {
            setup_logging(cli.config.as_deref(), cli.log_level.as_deref(), console)?;
            let reason = if handover {
                crate::win::StopReason::Handover
            } else {
                crate::win::StopReason::Restore
            };
            service::stop(reason)?;
            return Ok(());
        }
        Some(Command::Update { check }) => {
            setup_logging(cli.config.as_deref(), cli.log_level.as_deref(), console)?;
            return update_command(check);
        }
        Some(Command::Check { path, pid }) => return check(path.as_deref(), pid),
        Some(Command::Purge { yes }) => return purge_command(cli.config, yes),
        _ => {}
    }

    let path = resolve_config_path(cli.config)?;
    // The watcher loads the file itself: one it cannot use is shown in the
    // icon and waited on, not a reason to exit. The commands below need a
    // usable one and say so with the exit code.
    if matches!(cli.command, None | Some(Command::Run { .. })) {
        return service::serve(&path, cli.log_level.as_deref(), console);
    }
    let config = Config::load(&path)?;
    let level = cli
        .log_level
        .unwrap_or_else(|| config.general.log_level.clone());

    match cli.command {
        Some(Command::Validate) => {
            println!("Configuration `{}` is valid.", path.display());
            Ok(())
        }
        Some(Command::Status) => {
            logging::init(&level, None, console)?;
            status()
        }
        Some(Command::Trigger { event }) => {
            logging::init(&level, None, console)?;
            // The commands and nothing else: no detection, no session, no
            // marker. A trigger is for testing what the commands do.
            let (label, actions) = match event {
                TriggerEvent::Start => ("game_start", &config.on_game_start),
                TriggerEvent::Stop => ("game_stop", &config.on_game_stop),
            };
            actions::run_all(actions, &actions::ActionContext::new(label, None));
            Ok(())
        }
        _ => unreachable!("every other command returned above"),
    }
}

/// `update`: the same object the menu drives, from a console. The log
/// lines say what happens; the printed lines say what to do next.
fn update_command(check_only: bool) -> Result<()> {
    use crate::update::{Context, Launched, Verdict, Version, check_now, install_now, wait_for};

    let context = Context::of_this_process(None, None)?;
    let release = match check_now(&context) {
        Ok(Verdict::UpToDate) => {
            println!("{} is the latest version.", Version::running());
            return Ok(());
        }
        Ok(Verdict::Available(release)) => release,
        Err(fault) => anyhow::bail!("could not check for updates: {fault}"),
    };
    println!(
        "{} is available ({}); this is {}.",
        release.version,
        release.page,
        Version::running()
    );
    if check_only {
        return Ok(());
    }
    match install_now(&context, &release) {
        Ok(Launched::Installer(child)) => match wait_for(&context, child) {
            None => {
                println!(
                    "Installed {}; a watcher that was running is back on it.",
                    release.version
                );
                Ok(())
            }
            Some(fault) => anyhow::bail!("the update to {} failed: {fault}", release.version),
        },
        Ok(Launched::Shell) => {
            println!(
                "The update to {} continues once this command has exited; the log says how it went.",
                release.version
            );
            Ok(())
        }
        Err(fault) => anyhow::bail!("the update to {} failed: {fault}", release.version),
    }
}

/// The log for the setup commands: the configuration's level and folder
/// when there is a usable configuration, the defaults otherwise -- `init`
/// runs before any configuration exists, and a broken one is no reason to
/// lose the line that says what was done.
fn setup_logging(explicit: Option<&Path>, level: Option<&str>, console: bool) -> Result<()> {
    let config = resolve_config_path(explicit.map(Path::to_path_buf))
        .ok()
        .and_then(|path| Config::load(&path).ok());
    let level = level
        .map(str::to_owned)
        .or_else(|| {
            config
                .as_ref()
                .map(|config| config.general.log_level.clone())
        })
        .unwrap_or_else(|| "info".to_owned());
    let dir = config
        .as_ref()
        .and_then(|config| config.general.log_dir.clone())
        .or_else(|| config::local_dir().map(|dir| dir.join("logs")));
    logging::init(&level, dir.as_deref(), console)
}

fn resolve_config_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    match explicit {
        Some(path) => Ok(path),
        None => config::default_path(),
    }
}

/// Everything the program left on this machine, removed on request.
///
/// The configuration is read for the log's location and nothing else, so a
/// broken one does not stop the purge -- a broken configuration is a fine
/// reason to want one.
fn purge_command(explicit_config: Option<PathBuf>, yes: bool) -> Result<()> {
    let path = resolve_config_path(explicit_config)?;
    let config = Config::load(&path).ok();

    // The one refusal: a purge mid-game would leave the gaming configuration
    // on with nothing left to restore it.
    if let Ok(exe) = presence_writer::registered_exe()
        && presence_writer::running_pid(&exe).is_some()
    {
        anyhow::bail!("a game is running; quit it first, so its stop commands can run");
    }

    let plan = purge::Plan::compute(&purge::discover(config.as_ref(), &path));
    if plan.is_empty() {
        println!("Nothing of GameModeExecutor was found on this machine.");
        return Ok(());
    }
    println!("This will:");
    for line in plan.describe() {
        println!("  - {line}");
    }
    if !yes {
        print!("Type yes to continue: ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if answer.trim() != "yes" {
            println!("Nothing was changed.");
            return Ok(());
        }
    }
    purge::execute(&plan)
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

fn status() -> Result<()> {
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
    Ok(())
}

fn check(path: Option<&str>, pid: Option<u32>) -> Result<()> {
    let known = KnownGames::load()?;

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
fn print_foreground(snapshot: &Snapshot, known: Option<&KnownGames>) {
    let Some(pid) = detect::process::foreground_pid() else {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("gamemode-executor").chain(line.iter().copied()))
            .expect("parses")
    }

    #[test]
    fn the_default_command_is_run_and_hidden_is_still_accepted() {
        assert!(parse(&[]).command.is_none());
        assert!(matches!(
            parse(&["run", "--hidden"]).command,
            Some(Command::Run { hidden: true })
        ));
        assert!(matches!(
            parse(&["--config", r"C:\x\config.toml"]).config,
            Some(path) if path.ends_with("config.toml")
        ));
    }

    #[test]
    fn the_setup_commands_take_their_flags() {
        assert!(matches!(
            parse(&["stop", "--handover"]).command,
            Some(Command::Stop { handover: true })
        ));
        assert!(matches!(
            parse(&["stop"]).command,
            Some(Command::Stop { handover: false })
        ));
        assert!(matches!(
            parse(&["init", "--force"]).command,
            Some(Command::Init { force: true })
        ));
        match parse(&["install-task", "--delay", "1m", "--force"]).command {
            Some(Command::InstallTask { delay, force }) => {
                assert_eq!(delay, "1m");
                assert!(force);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse(&["uninstall-task"]).command,
            Some(Command::UninstallTask)
        ));
        assert!(matches!(
            parse(&["update", "--check"]).command,
            Some(Command::Update { check: true })
        ));
        assert!(matches!(
            parse(&["purge", "--yes"]).command,
            Some(Command::Purge { yes: true })
        ));
    }

    #[test]
    fn check_takes_a_path_or_a_pid_but_not_both() {
        assert!(matches!(
            parse(&["check", "--pid", "42"]).command,
            Some(Command::Check {
                path: None,
                pid: Some(42)
            })
        ));
        assert!(
            Cli::try_parse_from(["gamemode-executor", "check", r"C:\g.exe", "--pid", "1"]).is_err()
        );
    }

    /// The diagnostics against this machine: they read the Known Game List
    /// and the Game Bar registration, which a GitHub-hosted runner does not
    /// have, so they run where a Windows client is -- the script runs them
    /// when `CI` is not set.
    #[test]
    #[ignore = "reads this machine's registry, which a stock runner lacks"]
    fn status_and_check_answer_for_this_machine() {
        status().expect("status reports what it sees");
        check(Some(r"C:\Windows\notepad.exe"), None).expect("an executable is checked");
        check(None, Some(std::process::id())).expect("this process is inspected");
    }

    #[test]
    fn the_configuration_path_is_the_explicit_one_when_given() {
        let explicit = PathBuf::from(r"C:\somewhere\config.toml");
        assert_eq!(
            resolve_config_path(Some(explicit.clone())).unwrap(),
            explicit
        );
    }
}
