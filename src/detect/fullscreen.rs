//! Shell state, kept for diagnostics only.
//!
//! `SHQueryUserNotificationState` reports whether a full-screen application
//! owns the desktop. It was the interim detector before the presence writer
//! signal existed; it is a heuristic (a full-screen video player looks like a
//! game to it) so it no longer drives anything. `status` still prints it,
//! because it is useful context when a detection looks wrong.

use anyhow::{Result, bail};
use windows::Win32::UI::Shell::{
    QUERY_USER_NOTIFICATION_STATE, QUNS_APP, QUNS_BUSY, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Only meaningful in an interactive session: it fails from session 0.
pub fn notification_state() -> Result<QUERY_USER_NOTIFICATION_STATE> {
    // SAFETY: no arguments; the API only produces its return value.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => Ok(state),
        Err(error) => bail!("SHQueryUserNotificationState failed: {error}"),
    }
}

pub fn state_label(state: QUERY_USER_NOTIFICATION_STATE) -> &'static str {
    match state {
        QUNS_BUSY => "busy",
        QUNS_RUNNING_D3D_FULL_SCREEN => "d3d_exclusive",
        QUNS_PRESENTATION_MODE => "presentation",
        QUNS_APP => "store_app",
        _ => "none",
    }
}

/// Process owning the foreground window.
pub fn foreground_pid() -> Option<u32> {
    // SAFETY: no arguments and no preconditions; a null handle is checked.
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer. A window that vanished since the
    // call above makes the API return 0, which leaves `pid` untouched.
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}
