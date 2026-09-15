//! Running the configured executables.

use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::config::{Action, Event, Mode};
use crate::detect::GameSignal;
use crate::logging::target;

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
/// the user saying they do not want one. So `confirmed` is the strongest thing
/// that can honestly be said -- "at least one command was checked, and every
/// checked command succeeded" -- and not "nothing went wrong". A command that
/// could not be started counts as checked and failed whether or not it would
/// have been waited for, since that verdict is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcome {
    pub checked: usize,
    pub failed: usize,
}

impl Outcome {
    pub fn confirmed(&self) -> bool {
        self.checked > 0 && self.failed == 0
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

    let mut command = Command::new(&program);
    command.args(&args);
    if action.no_window {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    if let Some(dir) = &action.working_dir {
        command.current_dir(context.render(&dir.to_string_lossy()));
    }
    for (key, value) in &action.env {
        command.env(key, context.render(value));
    }

    tracing::debug!(target: target::COMMANDS, program, args = ?args, "Starting a command");
    command
        .spawn()
        .with_context(|| format!("cannot start `{program}`"))
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

    #[test]
    fn nothing_checked_confirms_nothing() {
        // Every command fire-and-forget: there is no verdict to lean on, so
        // a session-end stop cannot be called confirmed.
        let mut outcome = Outcome::default();
        outcome.record(None);
        outcome.record(None);
        assert!(!outcome.confirmed());
    }

    #[test]
    fn one_failure_among_checked_commands_denies_confirmation() {
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
        assert!(!outcome.confirmed());
    }

    #[test]
    fn checked_successes_confirm_regardless_of_unchecked_ones() {
        // The shape of the FanControl recipe: the profile switch is waited
        // for, the beep is not. The beep's fate does not count either way.
        let mut outcome = Outcome::default();
        outcome.record(Some(true));
        outcome.record(None);
        assert!(outcome.confirmed());
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

    #[test]
    fn missing_signal_renders_empty_placeholders() {
        let context = ActionContext::new("game_stop", None);
        assert_eq!(context.render("{event}/{process_name}"), "game_stop/");
    }
}
