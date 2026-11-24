//! Running the configured executables.
//!
//! A program named without a folder -- `schtasks.exe`, `powershell.exe` --
//! is looked for here, not by std: `Command::new` given a bare name searches
//! the running executable's own folder before the system folder and `PATH`,
//! so a copy unpacked in Downloads would run a `schtasks.exe` lying beside
//! it at every game start and stop. `locate` keeps std's order without
//! that folder: the `PATH` the command's own `env` sets, if any; the system
//! folder; the Windows folder; this process's `PATH`. Only absolute entries
//! of a `PATH` count, the current folder never does, and a name without an
//! extension gets `.exe`, std's rule. A program given with a folder is the
//! person's explicit choice and starts as written.

use std::ffi::OsString;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::config::{Action, Event, Mode};
use crate::detect::GameSignal;
use crate::logging::target;
use crate::win;

/// CREATE_NO_WINDOW: no console window for the child process.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Values substituted into program, args, working_dir and env.
#[derive(Debug, Clone, Default)]
pub struct ActionContext {
    pub event: String,
    pub process_name: String,
    pub process_id: String,
    pub process_path: String,
}

impl ActionContext {
    pub fn new(event: &str, signal: Option<&GameSignal>) -> Self {
        Self {
            event: event.to_owned(),
            process_name: signal
                .and_then(|signal| signal.process_name.clone())
                .unwrap_or_default(),
            process_id: signal
                .and_then(|signal| signal.process_id)
                .map(|pid| pid.to_string())
                .unwrap_or_default(),
            process_path: signal
                .and_then(|signal| signal.process_path.clone())
                .unwrap_or_default(),
        }
    }

    fn render(&self, template: &str) -> String {
        template
            .replace("{event}", &self.event)
            .replace("{process_name}", &self.process_name)
            .replace("{process_id}", &self.process_id)
            .replace("{process_path}", &self.process_path)
    }
}

/// What `run_all` can vouch for afterwards.
///
/// Only a command that was waited for has a verdict; a fire-and-forget one is
/// the user saying they do not want one. A command that could not be started
/// counts as checked and failed whether or not it would have been waited for,
/// since that verdict is in. The engine acts on one thing only: a failure,
/// after which the next start runs the stop commands again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcome {
    pub checked: usize,
    pub failed: usize,
}

impl Outcome {
    pub fn any_failed(&self) -> bool {
        self.failed > 0
    }

    fn record(&mut self, verdict: Option<bool>) {
        if let Some(succeeded) = verdict {
            self.checked += 1;
            if !succeeded {
                self.failed += 1;
            }
        }
    }
}

/// Run an event's commands, in series or all at once.
///
/// A command that fails to start is logged and never stops the others: an event
/// is a set of independent side effects, not a pipeline.
pub fn run_all(event: &Event, context: &ActionContext) -> Outcome {
    let mut outcome = Outcome::default();
    let actions: Vec<&Action> = event
        .actions
        .iter()
        .filter(|action| {
            if !action.enabled {
                tracing::debug!(target: target::COMMANDS, "Skipping disabled command `{}`", action.label());
            }
            action.enabled
        })
        .collect();

    if actions.is_empty() {
        return outcome;
    }
    tracing::debug!(
        target: target::COMMANDS,
        count = actions.len(),
        mode = event.mode.label(),
        "Running the configured commands"
    );

    match event.mode {
        Mode::Series => {
            for action in actions {
                let verdict = match start(action, context) {
                    Ok(child) => join(action, child),
                    Err(error) => {
                        tracing::error!(
                            target: target::COMMANDS,
                            error = %format!("{error:#}"),
                            "Command `{}` could not be started; check its program path in the configuration",
                            action.label()
                        );
                        Some(false)
                    }
                };
                outcome.record(verdict);
            }
        }
        Mode::Parallel => {
            let mut started = Vec::with_capacity(actions.len());
            for action in actions {
                match start(action, context) {
                    Ok(child) => started.push((action, child)),
                    Err(error) => {
                        tracing::error!(
                            target: target::COMMANDS,
                            error = %format!("{error:#}"),
                            "Command `{}` could not be started; check its program path in the configuration",
                            action.label()
                        );
                        outcome.record(Some(false));
                    }
                }
            }
            // Everything is running before anything is waited for. Waiting as
            // we started would have been series with extra steps.
            for (action, child) in started {
                outcome.record(join(action, child));
            }
        }
    }
    outcome
}

fn start(action: &Action, context: &ActionContext) -> Result<Child> {
    let program = context.render(&action.program.to_string_lossy());
    let args: Vec<String> = action.args.iter().map(|arg| context.render(arg)).collect();
    let env: Vec<(&String, String)> = action
        .env
        .iter()
        .map(|(key, value)| (key, context.render(value)))
        .collect();

    let search = Search {
        // Windows' variable names ignore case, and so does std's own check.
        command_path: env
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| OsString::from(value)),
        system: win::system_dir()?,
        windows: win::windows_dir()?,
        process_path: std::env::var_os("PATH"),
    };
    let path = locate(&program, &search, Path::is_file)?;

    let mut command = Command::new(&path);
    command.args(&args);
    if action.no_window {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    if let Some(dir) = &action.working_dir {
        command.current_dir(context.render(&dir.to_string_lossy()));
    }
    for (key, value) in env {
        command.env(key, value);
    }

    tracing::debug!(
        target: target::COMMANDS,
        program,
        path = %path.display(),
        args = %shown_arguments(&args),
        "Starting a command"
    );
    command
        .spawn()
        .with_context(|| format!("cannot start `{program}`"))
}

/// The folders a program named without a folder is looked for in.
struct Search {
    /// `PATH` as the command's own `env` sets it: searched first, as std
    /// does, since the person wrote it for this command.
    command_path: Option<OsString>,
    system: PathBuf,
    windows: PathBuf,
    /// This process's `PATH`, searched last.
    process_path: Option<OsString>,
}

impl Search {
    /// The folders in the order they are searched. A relative `PATH` entry
    /// -- `.`, `tools`, `\bin`, `C:bin` -- would be read against the current
    /// folder, which is the search this module exists to avoid.
    fn folders(&self) -> Vec<PathBuf> {
        let absolute = |paths: &Option<OsString>| -> Vec<PathBuf> {
            paths
                .iter()
                .flat_map(std::env::split_paths)
                .filter(|folder| folder.is_absolute())
                .collect()
        };
        let mut folders = absolute(&self.command_path);
        folders.push(self.system.clone());
        folders.push(self.windows.clone());
        folders.extend(absolute(&self.process_path));
        folders
    }
}

/// Where `program` starts from: as written when it names a folder, else
/// the first file `exists` finds in `search`'s folders.
fn locate(program: &str, search: &Search, exists: impl Fn(&Path) -> bool) -> Result<PathBuf> {
    if program.contains(['\\', '/']) {
        return Ok(PathBuf::from(program));
    }
    if program.is_empty() {
        anyhow::bail!("the program is empty once its placeholders are filled in");
    }
    // `CreateProcessW`'s rule, which std applies when it searches: a name
    // with no dot in it gets `.exe`.
    let name = if program.contains('.') {
        program.to_owned()
    } else {
        format!("{program}.exe")
    };
    search
        .folders()
        .into_iter()
        .map(|folder| folder.join(&name))
        .find(|candidate| exists(candidate))
        .with_context(|| {
            format!(
                "`{name}` is not in {}the system folder, the Windows folder or a folder of PATH, \
                 and it is never looked for beside GameModeExecutor",
                if search.command_path.is_some() {
                    "the command's PATH, "
                } else {
                    ""
                }
            )
        })
}

/// The arguments as a person would type them after the program, for the
/// log: one holding a space between quotes, nothing escaped, so a path
/// reads as Windows writes it. Not the command line `Command` builds.
fn shown_arguments(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.is_empty() || arg.contains(char::is_whitespace) {
                format!("\"{arg}\"")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `None` when the command was not waited for; otherwise whether it succeeded.
fn join(action: &Action, mut child: Child) -> Option<bool> {
    if !action.wait {
        return None;
    }
    let label = action.label();
    match wait_for(&mut child, action.timeout) {
        Ok(Some(status)) if status.success() => {
            tracing::debug!(target: target::COMMANDS, status = %status, "`{label}` finished");
            Some(true)
        }
        // A warning, because the user meets this as something that did not
        // happen -- a fan profile that stayed on -- and the log is where they
        // will look. Found the hard way: a logoff on 2026-09-16 left
        // `0xc0000142` at debug level and the fans on gaming settings.
        Ok(Some(status)) => {
            tracing::warn!(target: target::COMMANDS, status = %status, "`{label}` failed");
            Some(false)
        }
        Ok(None) => {
            tracing::warn!(
                target: target::COMMANDS,
                "`{label}` is still running after its timeout and was left to finish on its own"
            );
            Some(false)
        }
        Err(error) => {
            tracing::error!(
                target: target::COMMANDS,
                error = %format!("{error:#}"),
                "Lost track of `{label}` while waiting for it to finish"
            );
            Some(false)
        }
    }
}

/// Wait for the child process, giving up after the timeout when one is set.
fn wait_for(
    child: &mut std::process::Child,
    timeout: Option<Duration>,
) -> Result<Option<std::process::ExitStatus>> {
    let Some(timeout) = timeout else {
        return Ok(Some(child.wait()?));
    };
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cmd.exe /c exit N` is on every Windows machine, runners included, and
    /// exits with exactly the code asked for.
    fn exit_with(code: u8, wait: bool) -> Action {
        Action {
            name: Some(format!("exit {code}")),
            program: "cmd.exe".into(),
            args: vec!["/c".to_owned(), format!("exit {code}")],
            wait,
            timeout: Some(Duration::from_secs(10)),
            ..Action::default()
        }
    }

    fn event(mode: Mode, actions: Vec<Action>) -> Event {
        Event { mode, actions }
    }

    /// The recipe's own command, as the log shows it.
    #[test]
    fn the_arguments_read_in_the_log_as_typed() {
        let args = [r"/Run", r"/TN", r"GameModeExecutor\FanControl Game", ""].map(str::to_owned);
        assert_eq!(
            shown_arguments(&args),
            r#"/Run /TN "GameModeExecutor\FanControl Game" """#
        );
    }

    #[test]
    fn a_series_reports_each_waited_command_and_keeps_going_after_a_failure() {
        let outcome = run_all(
            &event(
                Mode::Series,
                vec![exit_with(0, true), exit_with(3, true), exit_with(0, true)],
            ),
            &ActionContext::default(),
        );
        assert_eq!(
            outcome,
            Outcome {
                checked: 3,
                failed: 1
            }
        );
        assert!(outcome.any_failed());
    }

    #[test]
    fn parallel_commands_are_all_waited_for() {
        let outcome = run_all(
            &event(Mode::Parallel, vec![exit_with(0, true), exit_with(0, true)]),
            &ActionContext::default(),
        );
        assert_eq!(
            outcome,
            Outcome {
                checked: 2,
                failed: 0
            }
        );
        assert!(!outcome.any_failed());
    }

    #[test]
    fn a_fire_and_forget_command_gives_no_verdict() {
        let outcome = run_all(
            &event(Mode::Series, vec![exit_with(1, false)]),
            &ActionContext::default(),
        );
        assert_eq!(outcome, Outcome::default());
        assert!(!outcome.any_failed());
    }

    #[test]
    fn a_program_that_cannot_start_is_a_failure_on_its_own() {
        let mut broken = exit_with(0, false);
        broken.program = "no-such-program-gamemode-executor.exe".into();
        let outcome = run_all(
            &event(Mode::Series, vec![broken, exit_with(0, true)]),
            &ActionContext::default(),
        );
        // The failure to start counts even though it was not waited for, and
        // the next command still ran.
        assert_eq!(
            outcome,
            Outcome {
                checked: 2,
                failed: 1
            }
        );
    }

    #[test]
    fn a_disabled_command_is_skipped() {
        let mut off = exit_with(7, true);
        off.enabled = false;
        let outcome = run_all(&event(Mode::Series, vec![off]), &ActionContext::default());
        assert_eq!(outcome, Outcome::default());
    }

    #[test]
    fn nothing_checked_is_no_failure() {
        // Every command fire-and-forget: nothing was seen to fail, so the
        // session closes. It would be wrong only at a session end, where the
        // commands die unseen, and there none is tried.
        let mut outcome = Outcome::default();
        outcome.record(None);
        outcome.record(None);
        assert!(!outcome.any_failed());
    }

    #[test]
    fn one_failure_among_checked_commands_is_a_failure() {
        let mut outcome = Outcome::default();
        outcome.record(Some(true));
        outcome.record(Some(false));
        assert_eq!(
            outcome,
            Outcome {
                checked: 2,
                failed: 1
            }
        );
        assert!(outcome.any_failed());
    }

    #[test]
    fn an_unchecked_command_never_counts_as_a_failure() {
        // The shape of the FanControl recipe: the configuration switch is
        // waited for, the beep is not. The beep's fate does not count.
        let mut outcome = Outcome::default();
        outcome.record(Some(true));
        outcome.record(None);
        assert!(!outcome.any_failed());
    }

    #[test]
    fn placeholders_are_substituted() {
        let signal = GameSignal {
            source: "process",
            process_name: Some("cs2.exe".to_owned()),
            process_id: Some(42),
            process_path: None,
        };
        let context = ActionContext::new("game_start", Some(&signal));
        assert_eq!(
            context.render("{event} {process_name} {process_id} [{process_path}]"),
            "game_start cs2.exe 42 []"
        );
    }

    /// Invented folders: `C:\Win` stands for the Windows folder, `C:\Copy`
    /// for the folder a copy was unpacked in, which no search may reach.
    fn search(command_path: Option<&str>, process_path: &str) -> Search {
        Search {
            command_path: command_path.map(OsString::from),
            system: PathBuf::from(r"C:\Win\System32"),
            windows: PathBuf::from(r"C:\Win"),
            process_path: Some(OsString::from(process_path)),
        }
    }

    /// An `exists` that knows exactly these files.
    fn only(files: &[&str]) -> impl Fn(&Path) -> bool {
        let files: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
        move |path| files.iter().any(|file| file == path)
    }

    #[test]
    fn a_bare_name_is_found_in_the_system_folder_before_path() {
        let found = locate(
            "schtasks.exe",
            &search(None, r"D:\Tools"),
            only(&[r"D:\Tools\schtasks.exe", r"C:\Win\System32\schtasks.exe"]),
        );
        assert_eq!(found.unwrap(), Path::new(r"C:\Win\System32\schtasks.exe"));
    }

    #[test]
    fn the_windows_folder_comes_after_the_system_folder_and_path_last() {
        let paths = search(None, r"D:\Tools").folders();
        assert_eq!(
            paths,
            [r"C:\Win\System32", r"C:\Win", r"D:\Tools"].map(PathBuf::from)
        );
        let found = locate(
            "tool.exe",
            &search(None, r"D:\Tools"),
            only(&[r"D:\Tools\tool.exe"]),
        );
        assert_eq!(found.unwrap(), Path::new(r"D:\Tools\tool.exe"));
    }

    #[test]
    fn the_copys_own_folder_and_the_current_folder_are_never_searched() {
        let error = locate(
            "schtasks.exe",
            &search(None, r".;Copy;\Copy;C:Copy"),
            only(&[
                r"C:\Copy\schtasks.exe",
                r".\schtasks.exe",
                r"Copy\schtasks.exe",
                r"\Copy\schtasks.exe",
                r"C:Copy\schtasks.exe",
            ]),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "`schtasks.exe` is not in the system folder, the Windows folder or a folder of PATH, \
             and it is never looked for beside GameModeExecutor"
        );
    }

    #[test]
    fn exe_is_added_only_to_a_name_without_an_extension() {
        let files = only(&[
            r"C:\Win\System32\powershell.exe",
            r"D:\Tools\sync.cmd",
            r"D:\Tools\my.tool",
        ]);
        let tools = search(None, r"D:\Tools");
        assert_eq!(
            locate("powershell", &tools, &files).unwrap(),
            Path::new(r"C:\Win\System32\powershell.exe")
        );
        assert_eq!(
            locate("sync.cmd", &tools, &files).unwrap(),
            Path::new(r"D:\Tools\sync.cmd")
        );
        assert_eq!(
            locate("my.tool", &tools, &files).unwrap(),
            Path::new(r"D:\Tools\my.tool")
        );
    }

    #[test]
    fn the_commands_own_path_is_searched_first() {
        let found = locate(
            "powershell.exe",
            &search(Some(r"E:\Mine"), r"D:\Tools"),
            only(&[r"E:\Mine\powershell.exe", r"C:\Win\System32\powershell.exe"]),
        );
        assert_eq!(found.unwrap(), Path::new(r"E:\Mine\powershell.exe"));
        let missing = locate(
            "absent.exe",
            &search(Some(r"E:\Mine"), r"D:\Tools"),
            only(&[]),
        );
        assert!(
            missing
                .unwrap_err()
                .to_string()
                .starts_with("`absent.exe` is not in the command's PATH, the system folder")
        );
    }

    #[test]
    fn a_quoted_path_entry_is_read_without_its_quotes() {
        let found = locate(
            "tool.exe",
            &search(None, r#""D:\Program Files\Tool";D:\Other"#),
            only(&[r"D:\Program Files\Tool\tool.exe"]),
        );
        assert_eq!(found.unwrap(), Path::new(r"D:\Program Files\Tool\tool.exe"));
    }

    #[test]
    fn a_program_with_a_folder_starts_as_written() {
        let never = |_: &Path| -> bool { panic!("a program with a folder is not searched for") };
        for program in [r"C:\Tools\fan.exe", r"tools\fan.exe", "./fan.exe"] {
            assert_eq!(
                locate(program, &search(None, r"D:\Tools"), never).unwrap(),
                Path::new(program)
            );
        }
    }

    #[test]
    fn an_empty_program_is_an_error() {
        assert!(locate("", &search(None, r"D:\Tools"), only(&[])).is_err());
    }

    #[test]
    fn missing_signal_renders_empty_placeholders() {
        let context = ActionContext::new("game_stop", None);
        assert_eq!(context.render("{event}/{process_name}"), "game_stop/");
    }
}
