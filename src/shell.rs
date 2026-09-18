//! Windows PowerShell in a window nobody sees, for the steps a process
//! cannot take itself: deleting its own executable, replacing it, waiting
//! for an installer that is about to stop it.
//!
//! Windows PowerShell rather than `cmd.exe`, because it can wait for exactly
//! one process -- `Wait-Process` on an id -- where a batch line could only
//! guess with a delay. `CREATE_NO_WINDOW` gives it a hidden console of its
//! own; outliving the process that started it needs no flag, Windows does
//! not end children with their parent. Only single quotes reach the command
//! line, so std's quoting for `CommandLineToArgvW` carries it through
//! intact. Seen the other way on 2026-09-17: a `cmd.exe` line with double
//! quotes, escaped as `\"` by std, deleted nothing.

use std::path::Path;
use std::process::{Child, Command};

use anyhow::{Context, Result};

/// A path as a PowerShell single-quoted literal, which only a quote can
/// end -- doubled inside, and nothing else expands.
pub fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

/// Run `script` in a hidden Windows PowerShell and hand back the process,
/// for a caller that wants to know when it is done -- or that does not,
/// and drops it.
pub fn hidden(script: &str) -> Result<Child> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
        ])
        .arg(script)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("cannot start the hidden shell")
}

/// Run `steps` once the process `pid` has exited. Each step says for itself
/// what it does when its target is already gone.
pub fn after_process(pid: u32, steps: &[String]) -> Result<()> {
    let mut script = vec![format!(
        "Wait-Process -Id {pid} -ErrorAction SilentlyContinue"
    )];
    script.extend(steps.iter().cloned());
    hidden(&script.join("; "))?;
    Ok(())
}

/// Run `steps` once this process has exited.
pub fn after_exit(steps: &[String]) -> Result<()> {
    after_process(std::process::id(), steps)
}
