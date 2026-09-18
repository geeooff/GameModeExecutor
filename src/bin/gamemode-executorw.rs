#![windows_subsystem = "windows"]
//! The same program with no console, for the logon task and the installer.
//!
//! The twin of `gamemode-executor.exe`: the same commands from the same
//! library, differing only in subsystem. The two subsystems cannot live in one
//! file, which is why Python ships `python.exe` and `pythonw.exe`, and why this
//! exists rather than a flag.
//!
//! What the subsystem changes is who waits and who reads. A shell does not
//! wait for a Windows-subsystem process, so a script that ran `validate`
//! through this binary would get its exit code before the verdict, and
//! nothing this binary prints goes anywhere. That is why people type the
//! console one and why the documentation only ever names it. This one is for
//! the two callers that have no console to give: the logon task, which runs
//! the watcher, and the installer, which runs `init` and `install-task` --
//! Windows Installer starts an executable action without hiding its console,
//! and the console binary flashed a window twice at the end of every install,
//! seen on 2026-09-18. The commands log what they did, so nothing is lost by
//! not printing it; the exit code is what the caller records.

use clap::Parser;

use game_mode_executor::{cli, exit};

fn main() -> std::process::ExitCode {
    match cli::run(cli::Cli::parse(), false) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            // There is no console to print to. The log file has the detail
            // when the command got as far as opening one, and the exit code
            // is what Task Scheduler and Windows Installer record.
            tracing::error!(
                target: game_mode_executor::logging::target::WATCHER,
                error = %format!("{error:#}"),
                "GameModeExecutor could not start"
            );
            std::process::ExitCode::from(exit::code_for(&error))
        }
    }
}
