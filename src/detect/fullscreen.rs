//! Detector based on the shell notification state, which tells whether a
//! full-screen application currently owns the desktop.

use anyhow::{Result, bail};
use windows_sys::Win32::UI::Shell::{
    QUERY_USER_NOTIFICATION_STATE, QUNS_APP, QUNS_BUSY, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use crate::config::FullscreenState;

use super::GameSignal;
use super::process::{Snapshot, full_path};

/// Wrapper over `SHQueryUserNotificationState`. Only meaningful in an
/// interactive session: it fails when called from session 0.
pub fn notification_state() -> Result<QUERY_USER_NOTIFICATION_STATE> {
    let mut state: QUERY_USER_NOTIFICATION_STATE = 0;
    let hresult = unsafe { SHQueryUserNotificationState(&mut state) };
    if hresult < 0 {
        bail!("SHQueryUserNotificationState failed (hresult 0x{hresult:08X})");
    }
    Ok(state)
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

fn matches(state: QUERY_USER_NOTIFICATION_STATE, wanted: &[FullscreenState]) -> bool {
    wanted.iter().any(|state_config| {
        let expected = match state_config {
            FullscreenState::D3dExclusive => QUNS_RUNNING_D3D_FULL_SCREEN,
            FullscreenState::Busy => QUNS_BUSY,
            FullscreenState::StoreApp => QUNS_APP,
            FullscreenState::Presentation => QUNS_PRESENTATION_MODE,
        };
        state == expected
    })
}

/// Process owning the foreground window, used to name the detected game.
pub fn foreground_pid() -> Option<u32> {
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    (pid != 0).then_some(pid)
}

#[derive(Debug)]
pub struct FullscreenDetector {
    states: Vec<FullscreenState>,
}

impl FullscreenDetector {
    pub fn new(states: &[FullscreenState]) -> Self {
        Self {
            states: states.to_vec(),
        }
    }

    pub fn detect(&self, snapshot: &Snapshot) -> Result<Option<GameSignal>> {
        let state = notification_state()?;
        if !matches(state, &self.states) {
            return Ok(None);
        }
        let pid = foreground_pid();
        Ok(Some(GameSignal {
            source: "fullscreen",
            process_name: pid
                .and_then(|pid| snapshot.by_pid(pid))
                .map(|process| process.name.clone()),
            process_id: pid,
            process_path: pid.and_then(full_path),
        }))
    }
}
