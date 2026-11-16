//! The logon task that started the watcher up to 0.4, now only removed.
//!
//! Since Lot 19 the watcher starts from the user's `Run` key (`startup`). A
//! copy upgraded from 0.4 still has its task, which would start a second
//! watcher at logon -- usually the first one, and the one Office's updates
//! end -- so `add-to-startup`, `remove-from-startup` and `purge` remove it.
//! Kept past 1.0.0, unlike the old verbs, decided 2026-11-16: it is what
//! protects a copy that skips versions.
//!
//! Before it goes, `add-to-startup` reads which configuration the task
//! passed, so a path chosen with 0.4's `install-task --config` survives the
//! upgrade, as Lot 8 promised. Read through Task Scheduler's own interface,
//! which answers in UTF-16: `schtasks /Query /XML` writes through the
//! console's code page, and a user name with an accent would not survive it.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{IExecAction, ITaskService, TaskScheduler};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, Interface};

/// The task an earlier version registered, folder included. The folder is
/// left in place when the task goes: the elevated tasks a recipe asks for
/// may live in it, and they are the user's.
pub const TASK_NAME: &str = r"GameModeExecutor\Watcher";

/// Whether that task is registered. `schtasks /Query` exits non-zero for a
/// task that does not exist, which is the whole answer.
pub fn exists() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// The configuration that task passes the watcher, if the task is there and
/// passes one. Anything that cannot be read is `None`: the caller then uses
/// the default, as it would without a task.
pub fn configuration() -> Option<PathBuf> {
    // SAFETY: no pointers; paired with the uninitialise below, on this thread.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    // SAFETY: every interface is a COM pointer the call before handed out and
    // windows-rs releases on drop, inside the closure; the result is plain
    // text by the time it leaves this statement, so nothing of COM outlives
    // it. The BSTRs and VARIANTs outlive the calls that read them.
    let arguments = unsafe {
        (|| -> windows::core::Result<String> {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
            let none = VARIANT::default();
            service.Connect(&none, &none, &none, &none)?;
            let (folder, name) = TASK_NAME.rsplit_once('\\').unwrap_or(("", TASK_NAME));
            let task = service
                .GetFolder(&BSTR::from(format!(r"\{folder}")))?
                .GetTask(&BSTR::from(name))?;
            // Task Scheduler counts actions from one.
            let action: IExecAction = task.Definition()?.Actions()?.get_Item(1)?.cast()?;
            let mut arguments = BSTR::new();
            action.Arguments(&mut arguments)?;
            Ok(arguments.to_string())
        })()
    }
    .ok();
    if com.is_ok() {
        // SAFETY: COM was initialised above on this thread, and every
        // interface obtained under it is released.
        unsafe { CoUninitialize() };
    }
    let path = config_argument(&arguments?)?;
    tracing::debug!(
        target: crate::logging::target::SETUP,
        task = TASK_NAME,
        config = %path.display(),
        "The logon task of an earlier version names this configuration, so the startup entry takes it"
    );
    Some(path)
}

/// The path after `--config` (or `-c`, or `--config=`) in a command line's
/// arguments, quoted or not, the way the watcher's own parser reads it.
fn config_argument(arguments: &str) -> Option<PathBuf> {
    let mut rest = arguments.trim_start();
    while !rest.is_empty() {
        let (token, after) = next_token(rest);
        if token == "--config" || token == "-c" {
            let (value, _) = next_token(after.trim_start());
            return (!value.is_empty()).then(|| PathBuf::from(value));
        }
        if let Some(value) = token.strip_prefix("--config=") {
            return (!value.is_empty()).then(|| PathBuf::from(value));
        }
        rest = after.trim_start();
    }
    None
}

/// One argument, its quotes taken off, and what follows it.
fn next_token(text: &str) -> (String, &str) {
    let mut token = String::new();
    let mut quoted = false;
    for (index, character) in text.char_indices() {
        match character {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => return (token, &text[index..]),
            c => token.push(c),
        }
    }
    (token, "")
}

/// Remove that task if it is there, saying so at `info`; nothing to say
/// when it is not.
pub fn remove_leftover() -> Result<()> {
    if !exists() {
        return Ok(());
    }
    let output = schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])
        .output()
        .context("cannot run schtasks.exe")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!("schtasks failed: {}", format!("{stdout}{stderr}").trim());
    }
    tracing::info!(
        target: crate::logging::target::SETUP,
        task = TASK_NAME,
        "Logon task of an earlier version removed; the startup entry starts the watcher now"
    );
    Ok(())
}

/// `schtasks.exe` with its output captured and no console window of its
/// own. A console program started from a parent that has no visible console
/// -- the installer's custom action, the windowless watcher -- opens one for
/// itself, and the user sees it flash: seen on 2026-09-18, on the first
/// install from the package.
fn schtasks(args: &[&str]) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = Command::new("schtasks");
    command.args(args).creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What 0.4's `install-task` wrote, and what a person may have typed.
    #[test]
    fn the_configuration_is_read_from_the_tasks_arguments() {
        let read =
            |arguments: &str| config_argument(arguments).map(|path| path.display().to_string());
        assert_eq!(
            read(r#"--config "C:\Users\Rémi\AppData\Roaming\GameModeExecutor\config.toml""#)
                .as_deref(),
            Some(r"C:\Users\Rémi\AppData\Roaming\GameModeExecutor\config.toml")
        );
        assert_eq!(
            read(r"--config C:\x\config.toml").as_deref(),
            Some(r"C:\x\config.toml")
        );
        assert_eq!(
            read(r#"-c "C:\A B\c.toml""#).as_deref(),
            Some(r"C:\A B\c.toml")
        );
        assert_eq!(
            read(r#"--config="C:\A B\c.toml""#).as_deref(),
            Some(r"C:\A B\c.toml")
        );
        assert_eq!(
            read(r#"run --hidden --config "C:\c.toml""#).as_deref(),
            Some(r"C:\c.toml")
        );
    }

    #[test]
    fn no_configuration_in_the_arguments_is_none() {
        assert_eq!(config_argument(""), None);
        assert_eq!(config_argument("--hidden"), None);
        assert_eq!(config_argument("--config"), None);
        assert_eq!(config_argument(r#"--config """#), None);
    }
}
