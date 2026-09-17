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

/// Task Scheduler folder everything this program installs lives in, rather than
/// scattered across the root alongside Windows' own tasks. `schtasks` creates
/// it on demand, so nothing has to make it first.
pub const TASK_FOLDER: &str = "GameModeExecutor";

/// The watcher's task, folder included. Named `Watcher` rather than repeating
/// the folder's name, so it reads as `GameModeExecutor \ Watcher` in the tree.
pub const TASK_NAME: &str = "GameModeExecutor\\Watcher";

/// The windowless twin this task is meant to run. Sits beside the console
/// binary, which is the one the user types and therefore the one running now.
const WATCHER_EXE: &str = "gamemode-executorw.exe";

/// What `install` did about the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    /// There was no task; there is one now.
    Registered,
    /// A task was there and was left alone.
    Kept,
    /// A task was there and `force` replaced it.
    Replaced,
}

/// The decision alone, so it can be tested without Task Scheduler.
pub fn registration(exists: bool, force: bool) -> Registration {
    match (exists, force) {
        (false, _) => Registration::Registered,
        (true, false) => Registration::Kept,
        (true, true) => Registration::Replaced,
    }
}

/// Register the logon task that starts the watcher with no console, then
/// start it, so the icon appears now rather than at the next logon. Runs
/// only while the user is logged on, so no password and no elevation.
///
/// A task that is already there is kept unless `force`: the installer calls
/// this on every install and upgrade, and must not undo a delay or a path
/// the user chose. Decided 2026-09-17, on the first install from the package.
/// Every outcome is logged at `info`, under `setup`.
pub fn install(config_path: &Path, delay: Duration, force: bool) -> Result<Registration> {
    let outcome = registration(exists(), force);
    if outcome == Registration::Kept {
        tracing::info!(
            target: crate::logging::target::SETUP,
            task = TASK_NAME,
            "Logon task kept: one is already registered (--force replaces it)"
        );
        start()?;
        return Ok(outcome);
    }
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

    match outcome {
        Registration::Registered => tracing::info!(
            target: crate::logging::target::SETUP,
            task = TASK_NAME,
            user = %user,
            program = %exe.display(),
            config = %config_path.display(),
            delay = ?delay,
            "Logon task registered: it starts the watcher at every logon, with no execution time limit"
        ),
        _ => tracing::info!(
            target: crate::logging::target::SETUP,
            task = TASK_NAME,
            user = %user,
            program = %exe.display(),
            config = %config_path.display(),
            delay = ?delay,
            "Logon task replaced, as asked"
        ),
    }
    start()?;
    Ok(outcome)
}

/// Run the task now. A watcher already running keeps the single-instance
/// mutex, so a second start exits at once and nothing doubles.
fn start() -> Result<()> {
    run_schtasks(&["/Run", "/TN", TASK_NAME])?;
    tracing::info!(
        target: crate::logging::target::SETUP,
        task = TASK_NAME,
        "Watcher started through its task; its icon appears in the notification area"
    );
    Ok(())
}

/// Remove the logon task. A task that is not there is not an error: the
/// outcome is logged either way.
pub fn uninstall() -> Result<()> {
    if !exists() {
        tracing::info!(
            target: crate::logging::target::SETUP,
            task = TASK_NAME,
            "No logon task to remove"
        );
        return Ok(());
    }
    run_schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])?;
    // The folder is left behind on purpose: anything else the user put in it --
    // the elevated tasks a recipe asks for, for instance -- is theirs, and
    // removing a folder that still holds their work would be worse than
    // leaving an empty one they can delete in a click.
    tracing::info!(
        target: crate::logging::target::SETUP,
        task = TASK_NAME,
        folder = TASK_FOLDER,
        "Logon task removed; the folder in Task Scheduler is left in place, empty or not"
    );
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
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let here = std::env::current_dir().context("cannot read the current directory")?;
        here.join(path)
    };
    // `join` concatenates, it does not normalise: a path typed with forward
    // slashes keeps them, and the result is a command line reading
    // `C:\GameModeExecutor\.local/config.toml`. Windows accepts
    // it, a person reading the task's properties should not have to.
    // Re-collecting the components emits the platform separator throughout and
    // drops any `.` along the way.
    Ok(joined.components().collect())
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

/// Whether the logon task is registered. `schtasks /Query` exits non-zero
/// for a task that does not exist, which is the whole answer.
pub fn exists() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
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

fn run_schtasks(args: &[&str]) -> Result<()> {
    let output = schtasks(args).output().context("cannot run schtasks.exe")?;
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
    fn a_task_is_registered_once_and_replaced_only_on_request() {
        assert_eq!(registration(false, false), Registration::Registered);
        assert_eq!(registration(false, true), Registration::Registered);
        assert_eq!(registration(true, false), Registration::Kept);
        assert_eq!(registration(true, true), Registration::Replaced);
    }

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

    /// The URI has to carry the folder too, or the task registers at the path
    /// `/TN` asks for while describing itself as living somewhere else.
    #[test]
    fn the_task_lives_in_its_own_folder() {
        let xml = definition(
            r"C:\tools\gamemode-executorw.exe",
            r"C:\config.toml",
            r"PC\me",
            Duration::ZERO,
        );
        assert_eq!(TASK_NAME, format!(r"{TASK_FOLDER}\Watcher"));
        assert!(xml.contains(&format!(r"<URI>\{TASK_NAME}</URI>")), "{xml}");
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

    /// The task's command line is shown to the user in Task Scheduler, so it
    /// should not carry the separators of whatever shell registered it.
    #[test]
    fn separators_are_normalised() {
        let mixed = absolute(Path::new("./.local/config.toml")).unwrap();
        let shown = mixed.to_string_lossy();
        assert!(!shown.contains('/'), "{shown}");
        assert!(shown.ends_with(r"\.local\config.toml"), "{shown}");

        let absolute_but_mixed = absolute(Path::new("C:/elsewhere/config.toml")).unwrap();
        assert_eq!(
            absolute_but_mixed,
            Path::new(r"C:\elsewhere\config.toml"),
            "{}",
            absolute_but_mixed.display()
        );
    }

    #[test]
    fn durations_become_iso8601() {
        assert_eq!(iso8601(Duration::from_secs(15)), "PT15S");
        assert_eq!(iso8601(Duration::ZERO), "PT0S");
    }
}
