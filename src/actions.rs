//! Running the configured executables.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::config::Action;
use crate::detect::GameSignal;

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

/// Run every enabled action in order. A failing action is logged and does not
/// prevent the following ones from running.
pub fn run_all(actions: &[Action], context: &ActionContext) {
    for action in actions {
        if !action.enabled {
            tracing::debug!("skipping disabled action `{}`", action.label());
            continue;
        }
        if let Err(error) = run_one(action, context) {
            tracing::error!("action `{}` failed: {error:#}", action.label());
        }
    }
}

fn run_one(action: &Action, context: &ActionContext) -> Result<()> {
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

    tracing::info!("running `{program}` {args:?}");
    let mut child = command
        .spawn()
        .with_context(|| format!("cannot start `{program}`"))?;

    if !action.wait {
        return Ok(());
    }

    match wait_for(&mut child, action.timeout)? {
        Some(status) => tracing::info!("`{program}` exited with {status}"),
        None => tracing::warn!("`{program}` still running after timeout, leaving it detached"),
    }
    Ok(())
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
