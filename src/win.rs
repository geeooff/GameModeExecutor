//! Thin, safe wrappers around the few Win32 calls the program needs.

use std::ffi::c_void;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WAIT_OBJECT_0,
    WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, INFINITE, SetEvent, WaitForSingleObject,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
    PostMessageW, PostQuitMessage, RegisterClassExW, TranslateMessage, WINDOW_EX_STYLE, WM_APP,
    WM_DESTROY, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSEXW, WS_OVERLAPPED,
};
use windows::core::{HSTRING, PCWSTR};

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

// ---------------------------------------------------------------------------
// The session window
// ---------------------------------------------------------------------------

/// Posted by the watcher thread once it has finished, so the message loop on
/// the main thread knows there is nothing left to wait for.
const WM_WATCHER_FINISHED: u32 = WM_APP + 1;

/// What the window procedure needs. There is exactly one watcher per process --
/// `SingleInstance` guarantees it -- so a process-wide slot is simpler and
/// safer than threading a raw pointer through `CREATESTRUCT`.
struct SessionState {
    stop: Arc<StopSignal>,
    finished: Arc<StopSignal>,
    grace: Duration,
}

static SESSION: OnceLock<SessionState> = OnceLock::new();

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // "Are you willing to let the session end?" Yes -- and start stopping
        // now, so the stop actions have the whole of Windows' own countdown
        // rather than only what is left of it at WM_ENDSESSION.
        WM_QUERYENDSESSION => {
            if let Some(state) = SESSION.get() {
                // Logged because the alternative is a log that simply stops.
                // This path runs once, unattended, on a machine nobody is
                // watching, and "Windows never asked" has to be tellable from
                // "we never answered".
                tracing::debug!(
                    target: crate::logging::target::WATCHER,
                    "Windows asked to end the session, so the watcher starts stopping now"
                );
                state.stop.signal();
            }
            LRESULT(1)
        }
        // "The session is ending." Hold this thread until the stop actions are
        // done, so the fan profile is restored before the process is killed.
        // This is the whole reason the window exists.
        WM_ENDSESSION => {
            if let Some(state) = SESSION.get().filter(|_| wparam.0 != 0) {
                let held = std::time::Instant::now();
                if state.finished.wait_timeout(state.grace) {
                    tracing::debug!(
                        target: crate::logging::target::WATCHER,
                        waited = ?held.elapsed(),
                        "The stop commands finished, the session may end"
                    );
                } else {
                    // The one case where a configured command does not get to
                    // run. Worth a warning rather than silence: the user meets
                    // it as a fan profile that stayed on gaming settings, and
                    // the next log they read should say why.
                    tracing::warn!(
                        target: crate::logging::target::WATCHER,
                        grace = ?state.grace,
                        "The stop commands did not finish before the session ended"
                    );
                }
            }
            LRESULT(0)
        }
        WM_WATCHER_FINISHED | WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // Anything else may belong to the notification icon, which hangs off
        // this window. It returns None for what it does not want, and that
        // falls through to Windows as usual.
        _ => match crate::tray::dispatch(message, wparam, lparam) {
            Some(result) => result,
            None => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        },
    }
}

/// A top-level window that is never shown.
///
/// It exists for one message: `WM_QUERYENDSESSION`. The console build learned
/// about logoff and shutdown through `ctrlc`, whose Windows handler signals on
/// every control event; a Windows-subsystem process with no window is simply
/// terminated instead, mid-game profile and all. This restores that behaviour.
///
/// **Not a message-only window.** Those are documented as not receiving
/// broadcast messages, and both `WM_QUERYENDSESSION` and the `WM_SETTINGCHANGE`
/// the tray icon will need are broadcasts. A top-level window that is never
/// shown costs the same and receives both.
pub struct SessionWindow {
    window: HWND,
}

impl SessionWindow {
    /// `grace` caps how long `WM_ENDSESSION` waits for the stop actions.
    pub fn create(
        stop: Arc<StopSignal>,
        finished: Arc<StopSignal>,
        grace: Duration,
    ) -> Result<Self> {
        let _ = SESSION.set(SessionState {
            stop,
            finished,
            grace,
        });

        let class_name = HSTRING::from("GameModeExecutorSession");
        let instance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;

        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        if unsafe { RegisterClassExW(&class) } == 0 {
            anyhow::bail!(
                "cannot register the session window class: {}",
                std::io::Error::last_os_error()
            );
        }

        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(class_name.as_ptr()),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .context("cannot create the session window")?;

        Ok(Self { window })
    }

    /// The handle as a plain integer. `HWND` is not `Send`; this is, so the
    /// watcher thread can carry it and post back.
    pub fn id(&self) -> isize {
        self.window.0 as isize
    }
}

impl Drop for SessionWindow {
    fn drop(&mut self) {
        unsafe { _ = DestroyWindow(self.window) };
    }
}

/// Tell Windows this process understands scaling, before any window exists.
///
/// Without it the process is DPI-unaware: `GetSystemMetrics` answers with the
/// 96 dpi values whatever the display is set to, so a notification icon is
/// built at 16 pixels and then stretched by the shell to the 24 a 150 % display
/// wants. The icon files carry a hand-tuned 24, and this is what lets Windows
/// be asked for it.
///
/// Failure is ignored on purpose: it means an older Windows, where the process
/// is DPI-unaware and the icon is merely soft.
pub fn declare_dpi_awareness() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// Pump messages until a quit is posted. Must run on the thread that created
/// the window, which is the main thread.
pub fn run_message_loop() {
    let mut message = MSG::default();
    loop {
        // 0 is WM_QUIT, -1 is an error. Both mean stop pumping.
        if unsafe { GetMessageW(&mut message, None, 0, 0) }.0 <= 0 {
            return;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Called from the watcher thread when it has finished, to let the message
/// loop return. `PostMessageW` is safe to call from any thread.
pub fn wake_message_loop(window: isize) {
    let window = HWND(window as *mut c_void);
    unsafe {
        let _ = PostMessageW(Some(window), WM_WATCHER_FINISHED, WPARAM(0), LPARAM(0));
    }
}

/// Carried as error context so the program can exit with a code that says a
/// second instance was refused, rather than a generic failure.
#[derive(Debug)]
pub struct AlreadyRunning;

impl std::fmt::Display for AlreadyRunning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "another instance is already running in this session")
    }
}

impl std::error::Error for AlreadyRunning {}

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
            return Err(anyhow::Error::new(AlreadyRunning));
        }
        Ok(Self { handle })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe { _ = CloseHandle(self.handle) };
    }
}
