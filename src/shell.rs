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
//!
//! Started by full path, `WindowsPowerShell\v1.0\powershell.exe` in the
//! system folder, where Windows keeps Windows PowerShell 5.1: given the bare
//! name, std would search this executable's own folder first, as
//! `Command::new` documents, and an unpacked copy may sit beside anything
//! there (`win::system_dir`).

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
    command(script)?
        .spawn()
        .context("cannot start the hidden shell")
}

/// The hidden Windows PowerShell for `script`, not yet started.
fn command(script: &str) -> Result<Command> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command =
        Command::new(crate::win::system_dir()?.join(r"WindowsPowerShell\v1.0\powershell.exe"));
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
        ])
        .arg(script)
        .creation_flags(CREATE_NO_WINDOW);
    Ok(command)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows' own, by a full path in the system folder, whatever sits
    /// beside the executable.
    #[test]
    fn the_shell_is_windows_powershell_by_full_path() {
        let command = command("exit").unwrap();
        let program = Path::new(command.get_program());
        assert!(program.starts_with(crate::win::system_dir().unwrap()));
        assert!(program.is_file(), "{}", program.display());
        assert!(program.ends_with("powershell.exe"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "exit"
            ]
        );
    }
}
