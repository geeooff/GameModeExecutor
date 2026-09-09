//! Thin, safe wrappers around the few Win32 calls the program needs.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Console::GetConsoleWindow;
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, INFINITE, SetEvent, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
use windows::core::{HSTRING, PCWSTR};

/// Hide the console window this process owns, if any. Used by `run --hidden`
/// so a logon-started instance does not leave a black window on screen.
pub fn hide_console() {
    let window = unsafe { GetConsoleWindow() };
    if !window.is_invalid() {
        let _ = unsafe { ShowWindow(window, SW_HIDE) };
    }
}

/// A manual-reset event used to unblock every wait in the program at once.
///
/// Waiting on a kernel event rather than checking a flag on a timer is what
/// keeps the watcher at zero wake-ups while a game is running.
pub struct StopSignal {
    event: HANDLE,
}

// A Win32 event handle is safe to signal and wait on from any thread.
unsafe impl Send for StopSignal {}
unsafe impl Sync for StopSignal {}

impl StopSignal {
    pub fn new() -> Result<Self> {
        let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
            .context("cannot create the stop event")?;
        Ok(Self { event })
    }

    pub fn signal(&self) {
        let _ = unsafe { SetEvent(self.event) };
    }

    pub fn is_set(&self) -> bool {
        unsafe { WaitForSingleObject(self.event, 0) == WAIT_OBJECT_0 }
    }

    /// Wait up to `timeout`. Returns true when the stop was signalled, which
    /// callers treat as "give up and return".
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let millis = timeout.as_millis().min(u128::from(INFINITE - 1)) as u32;
        unsafe { WaitForSingleObject(self.event, millis) == WAIT_OBJECT_0 }
    }

    pub(crate) fn handle(&self) -> HANDLE {
        self.event
    }
}

impl Drop for StopSignal {
    fn drop(&mut self) {
        unsafe { _ = CloseHandle(self.event) };
    }
}

/// Named mutex kept alive for the lifetime of the process, so a second
/// instance can detect the first one and bail out.
pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    /// Acquire the session-local mutex `name`, or fail if another process holds it.
    pub fn acquire(name: &str) -> Result<Self> {
        let name = HSTRING::from(format!("Local\\{name}"));
        let handle = unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) }
            .context("CreateMutexW failed")?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { _ = CloseHandle(handle) };
            bail!("another instance is already running in this session");
        }
        Ok(Self { handle })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe { _ = CloseHandle(self.handle) };
    }
}
