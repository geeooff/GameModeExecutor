//! The Game Bar presence writer, observed rather than replaced.
//!
//! Windows activates a WinRT server whenever it decides a game's presence
//! changed. Which executable that is comes from the registry, so a machine
//! where something else has taken over the registration is probed correctly
//! instead of us assuming the shipped binary.
//!
//! Writing that registration is not possible: the key is owned by
//! `NT SERVICE\TrustedInstaller` and even SYSTEM only holds `ReadKey`. Reading
//! it needs no privileges at all.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::registry::Key;

/// The out-of-proc server registration Windows resolves the class through.
const SERVER_KEY: &str = r"SOFTWARE\Microsoft\WindowsRuntime\Server\Windows.Gaming.GameBar.Internal.PresenceWriterServer";
const EXE_PATH_VALUE: &str = "ExePath";

/// What Windows currently ships, used only to report that the registration
/// looks unusual, never as a hard-coded target.
pub const MICROSOFT_DEFAULT: &str = r"C:\Windows\System32\GameBarPresenceWriter.exe";

/// The runtime class Windows activates on a presence change.
pub const CLASS_ID: &str = "Windows.Gaming.GameBar.PresenceServer.Internal.PresenceWriter";

/// The executable registered as the presence writer on this machine.
pub fn registered_exe() -> Result<PathBuf> {
    let key = Key::open_local_machine(SERVER_KEY)
        .context("the presence writer server is not registered on this machine")?;
    let path = key
        .string_value(EXE_PATH_VALUE)
        .with_context(|| format!("{SERVER_KEY}\\{EXE_PATH_VALUE} is empty"))?;
    Ok(PathBuf::from(path))
}

/// Is the registration still the one Microsoft ships?
pub fn is_microsoft_default(exe: &Path) -> bool {
    exe.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(MICROSOFT_DEFAULT)
}

/// PID of the registered presence writer, if it is running.
///
/// Matching is on the full image path, so an unrelated process that merely
/// shares the file name is not mistaken for it. When the image path cannot be
/// read the file name alone is accepted, which is the best we can do.
pub fn running_pid(exe: &Path) -> Option<u32> {
    find_in(&super::process::Snapshot::take().ok()?, exe)
}

/// Same, against a snapshot the caller already has. Useful when one tick needs
/// to look at several processes and should not pay for a snapshot each time.
pub fn find_in(snapshot: &super::process::Snapshot, exe: &Path) -> Option<u32> {
    let file_name = exe.file_name()?.to_string_lossy().to_ascii_lowercase();
    let expected = exe.to_string_lossy().to_ascii_lowercase();

    for process in &snapshot.processes {
        if !process.name.eq_ignore_ascii_case(&file_name) {
            continue;
        }
        match super::process::full_path(process.pid) {
            Some(path) if path.to_ascii_lowercase() == expected => return Some(process.pid),
            // Same name somewhere else on disk: not the registered writer.
            Some(_) => continue,
            None => return Some(process.pid),
        }
    }
    None
}

/// Why a wait on the writer process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// The process waited on exited: the writer, which Windows released,
    /// or the game marked by hand, which quit.
    Exited,
    /// The program was asked to shut down.
    Stopped,
    /// The requested time passed and the process is still alive.
    TimedOut,
}

/// Block until the writer exits or the program is stopped.
///
/// This is the whole point of the design: while a game runs there is no
/// polling at all, just a thread parked in the kernel on two handles.
/// Same, giving up after `timeout` and reporting `TimedOut`.
///
/// Used to do something partway through a session without giving up the
/// handle: a plain sleep would be blind to the game ending in the meantime.
pub fn wait_for_exit_until(
    pid: u32,
    stop: &crate::win::StopSignal,
    timeout: Option<std::time::Duration>,
) -> Result<WaitOutcome> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{
        INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForMultipleObjects,
    };

    // SAFETY: `OpenProcess` has no memory preconditions; a pid that no longer
    // exists makes it fail, which the `else` handles.
    let Ok(process) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
        // Already gone, or not ours to wait on: treat as exited rather than
        // spinning on a handle we cannot get.
        return Ok(WaitOutcome::Exited);
    };

    let millis = match timeout {
        Some(timeout) => timeout.as_millis().min(u128::from(INFINITE - 1)) as u32,
        None => INFINITE,
    };
    let mut handles = vec![process];
    handles.extend(stop.handles());
    // SAFETY: every handle is valid for the whole wait -- `process` was just
    // opened and is closed only afterwards, and the stop events live as long
    // as `stop`.
    let result = unsafe { WaitForMultipleObjects(&handles, false, millis) };
    // SAFETY: closes the handle opened above, exactly once.
    unsafe { _ = CloseHandle(process) };

    Ok(if result == WAIT_OBJECT_0 {
        WaitOutcome::Exited
    } else if result == WAIT_TIMEOUT {
        WaitOutcome::TimedOut
    } else {
        WaitOutcome::Stopped
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Same caveat as the Known Game List: Game Bar is a client feature, and
    // its registration does not exist on a Windows Server runner.
    #[test]
    #[ignore = "reads the Game Bar registration, absent on Windows Server runners"]
    fn the_registration_is_readable_without_elevation() {
        let exe = registered_exe().expect("presence writer registration");
        assert!(
            exe.as_os_str()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".exe"),
            "unexpected registration: {}",
            exe.display()
        );
    }

    #[test]
    fn the_shipped_default_is_recognised() {
        assert!(is_microsoft_default(Path::new(
            r"c:\windows\system32\gamebarpresencewriter.exe"
        )));
        assert!(!is_microsoft_default(Path::new(r"C:\Tools\other.exe")));
    }

    #[test]
    fn a_process_that_is_not_running_is_not_found() {
        assert_eq!(
            running_pid(Path::new(r"C:\Nowhere\definitely-not-running.exe")),
            None
        );
    }
}
