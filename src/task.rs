//! Registration as a per-user logon task.
//!
//! Registered from an XML definition rather than `schtasks` command-line flags,
//! because the defaults those flags leave behind are wrong for a watcher meant
//! to run forever: a 72 hour execution limit that kills it after three days,
//! and battery settings that stop it on an unplugged laptop.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

pub const TASK_NAME: &str = "GameModeExecutor";

/// The windowless twin this task is meant to run. Sits beside the console
/// binary, which is the one the user types and therefore the one running now.
const WATCHER_EXE: &str = "gamemode-executorw.exe";

/// Create (or replace) a logon task that starts the watcher with no console.
/// Runs only while the user is logged on, so no password and no elevation.
pub fn install(config_path: &Path, delay: Duration) -> Result<()> {
    let here = std::env::current_exe().context("cannot locate the running executable")?;
    let exe = here.with_file_name(WATCHER_EXE);
    if !exe.exists() {
        bail!(
            "`{}` is missing. It is built alongside this program and is the one the task \
             runs, because it has no console to leave on screen.",
            exe.display()
        );
    }
    // The task runs from whatever working directory Task Scheduler feels like,
    // so a relative path here would resolve at logon against somewhere else and
    // the watcher would exit 3 before anyone noticed. Absolute, always.
    let config_path = absolute(config_path)?;

    let user = current_user().context("cannot determine the current user")?;
    let xml = definition(
        &exe.to_string_lossy(),
        &config_path.to_string_lossy(),
        &user,
        delay,
    );

    let temp = std::env::temp_dir().join("GameModeExecutor-task.xml");
    write_utf16(&temp, &xml).with_context(|| format!("cannot write `{}`", temp.display()))?;
    let result = run_schtasks(&[
        "/Create",
        "/TN",
        TASK_NAME,
        "/XML",
        &temp.to_string_lossy(),
        "/F",
    ]);
    let _ = std::fs::remove_file(&temp);
    result?;

    println!("Scheduled task `{TASK_NAME}` created for {user}.");
    println!("  program : {}", exe.display());
    println!("  config  : {}", config_path.display());
    println!("  delay   : {delay:?} after logon, no execution time limit");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    run_schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])?;
    println!("Scheduled task `{TASK_NAME}` deleted.");
    Ok(())
}

fn definition(exe: &str, config: &str, user: &str, delay: Duration) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Watches for a running game and runs the configured commands. Started at logon, unelevated.</Description>
    <URI>\{name}</URI>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
      <Delay>{delay}</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>--config "{config}"</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        name = escape(TASK_NAME),
        user = escape(user),
        delay = iso8601(delay),
        exe = escape(exe),
        config = escape(config),
    )
}

/// Make a path absolute without requiring it to exist, and without the `\\?\`
/// prefix `canonicalize` adds -- Task Scheduler shows the command line to the
/// user, and that prefix is noise in it.
fn absolute(path: &Path) -> Result<std::path::PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let here = std::env::current_dir().context("cannot read the current directory")?;
    Ok(here.join(path))
}

/// Task Scheduler durations are ISO 8601. Seconds are enough here.
fn iso8601(delay: Duration) -> String {
    format!("PT{}S", delay.as_secs())
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Task Scheduler expects UTF-16 with a BOM, as it exports itself.
fn write_utf16(path: &Path, text: &str) -> std::io::Result<()> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.replace('\n', "\r\n").encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(path, bytes)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_definition_disables_the_traps() {
        let xml = definition(
            r"C:\tools\gamemode-executor.exe",
            r"C:\config.toml",
            r"PC\me",
            Duration::from_secs(15),
        );
        // The three settings whose schtasks defaults break a long-running watcher.
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
        // And it must stay unelevated.
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains("<Delay>PT15S</Delay>"));
    }

    /// The task must run the windowless binary with no subcommand. `run` and
    /// `--hidden` belong to the console binary, and passing either here would
    /// make the task fail at every logon with an argument error nobody sees,
    /// because there is no console to see it in.
    #[test]
    fn the_task_passes_only_the_configuration() {
        let xml = definition(
            r"C:\tools\gamemode-executorw.exe",
            r"C:\config.toml",
            r"PC\me",
            Duration::from_secs(15),
        );
        assert!(
            xml.contains(r#"<Arguments>--config "C:\config.toml"</Arguments>"#),
            "{xml}"
        );
        assert!(!xml.contains("--hidden"), "{xml}");
    }

    #[test]
    fn xml_special_characters_are_escaped() {
        let xml = definition("C:\\a&b.exe", "C:\\<config>.toml", "PC\\me", Duration::ZERO);
        assert!(xml.contains("C:\\a&amp;b.exe"));
        assert!(xml.contains("&lt;config&gt;"));
    }

    /// A relative path would resolve at logon against Task Scheduler's own
    /// working directory, and the watcher would exit 3 with nobody watching.
    #[test]
    fn a_relative_configuration_path_is_made_absolute() {
        let here = std::env::current_dir().unwrap();
        let made = absolute(Path::new(".local/config.toml")).unwrap();
        assert!(made.is_absolute(), "{}", made.display());
        assert!(made.starts_with(&here), "{}", made.display());

        let already = Path::new(r"C:\elsewhere\config.toml");
        assert_eq!(absolute(already).unwrap(), already);
    }

    #[test]
    fn durations_become_iso8601() {
        assert_eq!(iso8601(Duration::from_secs(15)), "PT15S");
        assert_eq!(iso8601(Duration::ZERO), "PT0S");
    }
}
