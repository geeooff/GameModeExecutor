//! Registration as a per-user logon task, via schtasks.exe.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub const TASK_NAME: &str = "GameModeExecutor";

/// Create (or replace) a logon task that starts the watcher hidden.
/// Runs only while the user is logged on, so no password and no elevation.
pub fn install(config_path: &Path, delay: &str) -> Result<()> {
    let exe = std::env::current_exe().context("cannot locate the running executable")?;
    let command = format!(
        "\"{}\" run --hidden --config \"{}\"",
        exe.display(),
        config_path.display()
    );
    let user = current_user().context("cannot determine the current user")?;

    run_schtasks(&[
        "/Create", "/TN", TASK_NAME, "/TR", &command, "/SC", "ONLOGON", "/RU", &user, "/IT", "/RL",
        "LIMITED", "/DELAY", delay, "/F",
    ])?;

    println!("Scheduled task `{TASK_NAME}` created for {user}.");
    println!("  command: {command}");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    run_schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])?;
    println!("Scheduled task `{TASK_NAME}` deleted.");
    Ok(())
}

fn current_user() -> Option<String> {
    let name = std::env::var("USERNAME").ok()?;
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => Some(format!("{domain}\\{name}")),
        _ => Some(name),
    }
}

fn run_schtasks(args: &[&str]) -> Result<()> {
    let output = Command::new("schtasks")
        .args(args)
        .output()
        .context("cannot run schtasks.exe")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!("schtasks failed: {}", format!("{stdout}{stderr}").trim());
    }
    Ok(())
}
