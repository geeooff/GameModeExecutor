//! Starting the watcher at logon from the user's `Run` key.
//!
//! Windows starts every program named under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` when the user logs
//! on, and the Startup page of Settings, like Task Manager's, lists each one
//! with a switch. Up to 0.4 a logon task started the watcher instead; Office's
//! updates end what a logon task started and spare what the `Run` key started,
//! measured from 2026-10-17 to 2026-11-16 (`docs/design/19-startup-list.md`).
//!
//! The rules this module keeps:
//!
//! - The program is the one writer of its value, for the package and the zip
//!   alike, and a value already there is kept unless asked: it may name a
//!   configuration the user chose.
//! - The switch is Explorer's. Its entry under `StartupApproved\Run` is never
//!   written here, so a watcher switched off stays off across upgrades, and it
//!   is removed with the value, by name, so nothing of ours is left behind.
//! - The watcher started right after an install is started by Explorer,
//!   through the shell's automation object on the desktop, so it gets the
//!   parent a logon gives it: the one the measurement saw survive. A process
//!   this command started itself would be the installer's child, which
//!   nothing measured. If Explorer cannot be asked, the watcher waits for the
//!   next logon, said at `warn`: an install must not fail for it.
//!   <https://devblogs.microsoft.com/oldnewthing/20131118-00/?p=2643>
//! - The value is a command line of at most 260 characters, Microsoft's
//!   limit; a longer one is refused rather than written cut.
//!   <https://learn.microsoft.com/en-us/windows/win32/setupapi/run-and-runonce-registry-keys>

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoUninitialize, IDispatch, IServiceProvider,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    CSIDL_DESKTOP, IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellWindows,
    SID_STopLevelBrowser, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{BSTR, Interface};
use windows_registry::CURRENT_USER;

use crate::logging::target;

/// The value's name under the `Run` key. The Startup page shows the
/// executable's description instead, *GameModeExecutor watcher*.
pub const VALUE_NAME: &str = "GameModeExecutor";

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Where Explorer keeps the Startup page's switches for the `Run` key.
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// The windowless twin the entry starts. Sits beside whichever binary runs
/// this command: the console one a person typed, or the twin itself when
/// the installer runs it.
const WATCHER_EXE: &str = "gamemode-executorw.exe";

/// Microsoft's limit on the value's command line.
const MAX_COMMAND_LINE: usize = 260;

/// What `add` did about the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    /// There was no entry; there is one now.
    Added,
    /// An entry was there and was left alone.
    Kept,
    /// An entry was there and `force` replaced it.
    Replaced,
}

/// The decision alone, so it can be tested without the registry.
pub fn registration(exists: bool, force: bool) -> Registration {
    match (exists, force) {
        (false, _) => Registration::Added,
        (true, false) => Registration::Kept,
        (true, true) => Registration::Replaced,
    }
}

/// The entry as it stands, if there is one.
pub fn entry() -> Option<String> {
    CURRENT_USER
        .open(RUN_KEY)
        .ok()?
        .get_string(VALUE_NAME)
        .ok()
        .filter(|line| !line.is_empty())
}

/// Add the watcher to the programs Windows starts at logon, unless it is
/// there already, then have Explorer start it, so the icon appears now
/// rather than at the next logon. A watcher already running keeps the
/// single-instance mutex, so the second start exits at once.
///
/// The installer calls this on every install and upgrade, and the zip's
/// updater after every update. Every outcome is logged at `info`, under
/// `setup`.
pub fn add(config_path: &Path, force: bool) -> Result<Registration> {
    let existing = entry();
    let outcome = registration(existing.is_some(), force);
    let line = match (existing, outcome) {
        (Some(line), Registration::Kept) => {
            tracing::info!(
                target: target::SETUP,
                value = VALUE_NAME,
                command = %line,
                "Startup entry kept: one is already there (--force replaces it)"
            );
            line
        }
        _ => {
            let here = std::env::current_exe().context("cannot locate the running executable")?;
            let exe = here.with_file_name(WATCHER_EXE);
            if !exe.exists() {
                bail!(
                    "`{}` is missing. It is built alongside this program and is the one Windows \
                     starts at logon, because it has no console to leave on screen.",
                    exe.display()
                );
            }
            let line = command_line(&exe, &absolute(config_path)?)?;
            CURRENT_USER
                .create(RUN_KEY)
                .and_then(|key| key.set_string(VALUE_NAME, &line))
                .with_context(|| format!("cannot write `{VALUE_NAME}` under HKCU\\{RUN_KEY}"))?;
            if outcome == Registration::Added {
                tracing::info!(
                    target: target::SETUP,
                    value = VALUE_NAME,
                    command = %line,
                    "Added to the programs Windows starts at logon; Settings lists it under Startup, with a switch"
                );
            } else {
                tracing::info!(
                    target: target::SETUP,
                    value = VALUE_NAME,
                    command = %line,
                    "Startup entry replaced, as asked"
                );
            }
            line
        }
    };
    // A task left by 0.4 would start a second watcher at logon, and that one
    // first: worth a word, not worth failing an install over.
    if let Err(error) = crate::task::remove_leftover() {
        tracing::warn!(
            target: target::SETUP,
            task = crate::task::TASK_NAME,
            error = %format!("{error:#}"),
            "The logon task of an earlier version could not be removed, so it may start a second watcher at logon"
        );
    }
    start(&line);
    Ok(outcome)
}

/// Remove the watcher from the programs Windows starts at logon, and the
/// Startup page's switch for it. An entry that is not there is not an error.
pub fn remove() -> Result<()> {
    if entry().is_some() {
        CURRENT_USER
            .create(RUN_KEY)
            .and_then(|key| key.remove_value(VALUE_NAME))
            .with_context(|| format!("cannot remove `{VALUE_NAME}` from HKCU\\{RUN_KEY}"))?;
        tracing::info!(
            target: target::SETUP,
            value = VALUE_NAME,
            "Removed from the programs Windows starts at logon"
        );
    } else {
        tracing::info!(
            target: target::SETUP,
            value = VALUE_NAME,
            "No startup entry to remove"
        );
    }
    // Explorer writes the switch's entry when it first lists ours.
    if let Ok(key) = CURRENT_USER.options().read().write().open(APPROVED_KEY)
        && key.get_type(VALUE_NAME).is_ok()
    {
        key.remove_value(VALUE_NAME)
            .with_context(|| format!("cannot remove `{VALUE_NAME}` from HKCU\\{APPROVED_KEY}"))?;
        tracing::debug!(
            target: target::SETUP,
            value = VALUE_NAME,
            "The Startup page's switch for it removed"
        );
    }
    Ok(())
}

/// The value's data: the twin and the configuration it reads, both quoted.
fn command_line(exe: &Path, config: &Path) -> Result<String> {
    let line = format!("\"{}\" --config \"{}\"", exe.display(), config.display());
    let length = line.encode_utf16().count();
    if length > MAX_COMMAND_LINE {
        bail!(
            "the command line Windows would start at logon is {length} characters long, and \
             Windows reads at most {MAX_COMMAND_LINE}: `{line}`. A shorter path to the program \
             or to the configuration fits."
        );
    }
    Ok(line)
}

/// A command line split into the program and what follows it, the way the
/// shell takes them.
fn split(line: &str) -> (&str, &str) {
    let line = line.trim_start();
    if let Some(rest) = line.strip_prefix('"') {
        match rest.find('"') {
            Some(end) => (&rest[..end], rest[end + 1..].trim_start()),
            None => (rest, ""),
        }
    } else {
        match line.find(char::is_whitespace) {
            Some(end) => (&line[..end], line[end..].trim_start()),
            None => (line, ""),
        }
    }
}

/// Have Explorer start what the entry names, and say how it went.
fn start(line: &str) {
    let (program, parameters) = split(line);
    match start_from_explorer(program, parameters) {
        Ok(()) => tracing::info!(
            target: target::SETUP,
            program,
            "Watcher started through Explorer; its icon appears in the notification area"
        ),
        Err(error) => tracing::warn!(
            target: target::SETUP,
            program,
            error = %error,
            "The watcher could not be started now, so it starts at the next logon"
        ),
    }
}

/// `ShellExecute` run by Explorer rather than by this process: the desktop's
/// folder view, found through the shell's window list, hands out the shell
/// automation object of the Explorer that owns it, and what that object
/// starts is Explorer's child. Microsoft's documented interfaces, in the
/// order Raymond Chen gives them: the desktop's view found as in his post of
/// 2013-03-18, its automation object asked as in the one linked above.
fn start_from_explorer(program: &str, parameters: &str) -> windows::core::Result<()> {
    // SAFETY: no pointers; paired with the uninitialise below, on this thread.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let directory = Path::new(program)
        .parent()
        .map(|dir| dir.display().to_string())
        .unwrap_or_default();
    // SAFETY: every interface is a COM pointer the call before handed out and
    // windows-rs releases on drop, all of them inside this block, before COM
    // is uninitialised; the VARIANTs and the BSTR outlive the calls they are
    // passed to, which copy what they keep.
    let started = unsafe {
        (|| -> windows::core::Result<()> {
            let windows: IShellWindows =
                CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
            let location = VARIANT::from(CSIDL_DESKTOP as i32);
            let root = VARIANT::default();
            let mut window = 0;
            let desktop = windows.FindWindowSW(
                &location,
                &root,
                SWC_DESKTOP,
                &mut window,
                SWFO_NEEDDISPATCH,
            )?;
            let browser: IShellBrowser = desktop
                .cast::<IServiceProvider>()?
                .QueryService(&SID_STopLevelBrowser)?;
            let view = browser.QueryActiveShellView()?;
            let background: IDispatch = view.GetItemObject(SVGIO_BACKGROUND)?;
            let shell: IShellDispatch2 = background
                .cast::<IShellFolderViewDual>()?
                .Application()?
                .cast()?;
            shell.ShellExecute(
                &BSTR::from(program),
                &VARIANT::from(parameters),
                &VARIANT::from(directory.as_str()),
                &VARIANT::from("open"),
                &VARIANT::from(SW_SHOWNORMAL.0),
            )
        })()
    };
    if com.is_ok() {
        // SAFETY: COM was initialised above on this thread, and every
        // interface obtained under it has been released.
        unsafe { CoUninitialize() };
    }
    started
}

/// Make a path absolute without requiring it to exist, and without the
/// `\\?\` prefix `canonicalize` adds -- the Startup page and the registry
/// show the command line to the user, and that prefix is noise in it.
fn absolute(path: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let here = std::env::current_dir().context("cannot read the current directory")?;
        here.join(path)
    };
    // `join` concatenates, it does not normalise: a path typed with forward
    // slashes keeps them, and the value would read
    // `C:\GameModeExecutor\.local/config.toml`. Windows accepts it, a person
    // reading the entry should not have to. Re-collecting the components
    // emits the platform separator throughout and drops any `.` on the way.
    Ok(joined.components().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_is_added_once_and_replaced_only_on_request() {
        assert_eq!(registration(false, false), Registration::Added);
        assert_eq!(registration(false, true), Registration::Added);
        assert_eq!(registration(true, false), Registration::Kept);
        assert_eq!(registration(true, true), Registration::Replaced);
    }

    /// The entry runs the windowless binary with the configuration and
    /// nothing else: `run` is the default command, and an entry that names
    /// no flag keeps working whatever later versions do with them.
    #[test]
    fn the_entry_quotes_both_paths_and_passes_only_the_configuration() {
        let line = command_line(
            Path::new(r"C:\Users\A B\Programs\gamemode-executorw.exe"),
            Path::new(r"C:\Users\A B\config.toml"),
        )
        .unwrap();
        assert_eq!(
            line,
            r#""C:\Users\A B\Programs\gamemode-executorw.exe" --config "C:\Users\A B\config.toml""#
        );
    }

    #[test]
    fn a_command_line_past_microsofts_limit_is_refused() {
        let deep = format!(r"C:\{}\config.toml", "d".repeat(240));
        let error = command_line(Path::new(r"C:\gamemode-executorw.exe"), Path::new(&deep))
            .unwrap_err()
            .to_string();
        assert!(error.contains("at most 260"), "{error}");
    }

    #[test]
    fn a_command_line_splits_into_the_program_and_the_rest() {
        assert_eq!(
            split(r#""C:\A B\w.exe" --config "C:\A B\c.toml""#),
            (r"C:\A B\w.exe", r#"--config "C:\A B\c.toml""#)
        );
        assert_eq!(
            split(r"C:\w.exe --config C:\c.toml"),
            (r"C:\w.exe", r"--config C:\c.toml")
        );
        assert_eq!(split(r#""C:\A B\w.exe""#), (r"C:\A B\w.exe", ""));
        assert_eq!(split(r"C:\w.exe"), (r"C:\w.exe", ""));
    }

    /// A relative path would resolve at logon against Explorer's working
    /// directory, and the watcher would exit 3 with nobody watching.
    #[test]
    fn a_relative_configuration_path_is_made_absolute() {
        let here = std::env::current_dir().unwrap();
        let made = absolute(Path::new(".local/config.toml")).unwrap();
        assert!(made.is_absolute(), "{}", made.display());
        assert!(made.starts_with(&here), "{}", made.display());

        let already = Path::new(r"C:\elsewhere\config.toml");
        assert_eq!(absolute(already).unwrap(), already);
    }

    /// The command line is shown to the user, so it should not carry the
    /// separators of whatever shell added it.
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
}
