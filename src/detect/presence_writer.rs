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
    /// The writer exited: Windows released it, so the game session is over.
    WriterExited,
    /// The program was asked to shut down.
    Stopped,
}

/// Block until the writer exits or the program is stopped.
///
/// This is the whole point of the design: while a game runs there is no
/// polling at all, just a thread parked in the kernel on two handles.
pub fn wait_for_exit(pid: u32, stop: &crate::win::StopSignal) -> Result<WaitOutcome> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForMultipleObjects,
    };

    let Ok(process) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
        // Already gone, or not ours to wait on: treat as exited rather than
        // spinning on a handle we cannot get.
        return Ok(WaitOutcome::WriterExited);
    };

    let handles = [process, stop.handle()];
    let result = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
    unsafe { _ = CloseHandle(process) };

    Ok(if result == WAIT_OBJECT_0 {
        WaitOutcome::WriterExited
    } else {
        WaitOutcome::Stopped
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
