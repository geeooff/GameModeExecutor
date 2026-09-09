//! Thin, safe wrappers around the few Win32 calls the program needs.

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Console::GetConsoleWindow;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

/// Null-terminated UTF-16 buffer for a Win32 `PCWSTR` argument.
pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Hide the console window this process owns, if any. Used by `run --hidden`
/// so a logon-started instance does not leave a black window on screen.
pub fn hide_console() {
    let window = unsafe { GetConsoleWindow() };
    if !window.is_null() {
        unsafe { ShowWindow(window, SW_HIDE) };
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
        let name = wide(&format!("Local\\{name}"));
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        if handle.is_null() {
            bail!("CreateMutexW failed (error {})", unsafe { GetLastError() });
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            bail!("another instance is already running in this session");
        }
        Ok(Self { handle })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}
