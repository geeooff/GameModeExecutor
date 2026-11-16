//! The logon task that started the watcher up to 0.4, now only removed.
//!
//! Since Lot 19 the watcher starts from the user's `Run` key (`startup`). A
//! copy upgraded from 0.4 still has its task, which would start a second
//! watcher at logon -- usually the first one, and the one Office's updates
//! end -- so `add-to-startup`, `remove-from-startup` and `purge` remove it.
//! Kept past 1.0.0, unlike the old verbs, decided 2026-11-16: it is what
//! protects a copy that skips versions.

use std::process::Command;

use anyhow::{Context, Result, bail};

/// The task an earlier version registered, folder included. The folder is
/// left in place when the task goes: the elevated tasks a recipe asks for
/// may live in it, and they are the user's.
pub const TASK_NAME: &str = "GameModeExecutor\\Watcher";

/// Whether that task is registered. `schtasks /Query` exits non-zero for a
/// task that does not exist, which is the whole answer.
pub fn exists() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
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
