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
    CreateEventW, CreateMutexW, INFINITE, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE, SetEvent,
    WaitForSingleObject,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, GetClassNameW,
    GetMessageW, MSG, PostMessageW, PostQuitMessage, RegisterClassExW, TranslateMessage,
    WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSEXW,
    WS_OVERLAPPED,
};
use windows::core::{BOOL, HSTRING, PCWSTR};

/// A manual-reset event used to unblock every wait in the program at once.
///
/// Waiting on a kernel event rather than checking a flag on a timer is what
/// keeps the watcher at zero wake-ups while a game is running.
pub struct StopSignal {
    event: HANDLE,
}

// SAFETY: a Win32 event handle is a kernel object; signalling and waiting on
// it from any thread is what it is for, and the struct holds nothing else.
unsafe impl Send for StopSignal {}
// SAFETY: as above -- every method takes `&self` and the kernel serialises.
unsafe impl Sync for StopSignal {}

impl StopSignal {
    pub fn new() -> Result<Self> {
        // SAFETY: no security attributes, no name; the handle that comes back
        // is owned by `StopSignal` and closed on drop.
        let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
            .context("cannot create the stop event")?;
        Ok(Self { event })
    }

    pub fn signal(&self) {
        // SAFETY: the event is open for as long as `self` lives.
        let _ = unsafe { SetEvent(self.event) };
    }

    pub fn is_set(&self) -> bool {
        // SAFETY: as for `signal`.
        unsafe { WaitForSingleObject(self.event, 0) == WAIT_OBJECT_0 }
    }

    /// Wait up to `timeout`. Returns true when the stop was signalled, which
    /// callers treat as "give up and return".
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let millis = timeout.as_millis().min(u128::from(INFINITE - 1)) as u32;
        // SAFETY: as for `signal`.
        unsafe { WaitForSingleObject(self.event, millis) == WAIT_OBJECT_0 }
    }

    pub(crate) fn handle(&self) -> HANDLE {
        self.event
    }
}

impl Drop for StopSignal {
    fn drop(&mut self) {
        // SAFETY: the handle came from `CreateEventW` and is closed once.
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
            // SAFETY: no arguments beyond the exit code; only affects the
            // calling thread's message queue.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // Anything else may belong to the notification icon, which hangs off
        // this window. It returns None for what it does not want, and that
        // falls through to Windows as usual.
        _ => match crate::tray::dispatch(message, wparam, lparam) {
            Some(result) => result,
            // SAFETY: the arguments are exactly those Windows handed to this
            // procedure, forwarded unchanged.
            None => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        },
    }
}

/// The window class of the session window, which is how another process
/// of this program finds it.
const SESSION_CLASS: &str = "GameModeExecutorSession";

/// Ask a running watcher to quit, the way its *Quit* menu entry does: `WM_CLOSE`
/// on its session window, which the default procedure turns into
/// `WM_DESTROY` and so into the end of the message loop. Nothing here waits;
/// the caller watches the single-instance mutex to know the process is gone.
///
/// `FindWindowW` cannot see a class another process registered, so the
/// top-level windows are enumerated and asked their class name instead.
pub fn close_session_window() -> Result<()> {
    unsafe extern "system" fn visit(window: HWND, found: LPARAM) -> BOOL {
        let mut name = [0u16; 64];
        // SAFETY: `name` is a valid buffer and its length is what is passed;
        // GetClassNameW writes at most that many characters.
        let len = unsafe { GetClassNameW(window, &mut name) };
        if len > 0 && String::from_utf16_lossy(&name[..len as usize]) == SESSION_CLASS {
            // SAFETY: WM_CLOSE carries no pointers; the window handle came
            // from the enumeration and may be gone by the time it is read,
            // which PostMessageW reports rather than dereferences.
            if unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok() {
                // SAFETY: `found` is the address of the caller's `bool`,
                // alive for the whole enumeration.
                unsafe { *(found.0 as *mut bool) = true };
            }
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut found = false;
    // SAFETY: the callback reads only what it is given and writes only to
    // `found`, whose address is passed and which outlives the call. An
    // enumeration the callback stops is reported as an error by EnumWindows,
    // which is why its result is not the verdict.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut found as *mut bool as isize)) };
    anyhow::ensure!(found, "no running watcher was found");
    Ok(())
}

/// A top-level window that is never shown.
///
/// It was built for one message, `WM_QUERYENDSESSION`, to preserve what the
/// console build was believed to do at logoff through `ctrlc`. A real logoff
/// showed that a command started at that point cannot run -- see
/// `docs/design/05-windowless-watcher.md` -- so the handshake is kept for a
/// `Quit`, and the window earns its place as what the notification icon and
/// the theme broadcasts hang off.
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

        let class_name = HSTRING::from(SESSION_CLASS);
        // SAFETY: `None` asks for the calling executable's own module.
        let instance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;

        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        // SAFETY: `class` is fully initialised, `cbSize` included, and the
        // strings it points at outlive the registration.
        if unsafe { RegisterClassExW(&class) } == 0 {
            anyhow::bail!(
                "cannot register the session window class: {}",
                std::io::Error::last_os_error()
            );
        }

        // SAFETY: the class was registered just above with a valid procedure,
        // and `class_name` outlives the call. The window is destroyed on drop.
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
        // SAFETY: the window was created by this struct and is destroyed once,
        // on the thread that created it.
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
    // SAFETY: a process-wide setting with no pointer arguments; it fails
    // harmlessly if already set.
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
        // SAFETY: `message` is a valid out pointer for the calling thread's
        // queue.
        if unsafe { GetMessageW(&mut message, None, 0, 0) }.0 <= 0 {
            return;
        }
        // SAFETY: `message` was just filled in by `GetMessageW`.
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
    // SAFETY: posting to a handle carries no pointers; a window that no longer
    // exists makes the call fail, which is ignored.
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
        // SAFETY: `name` is NUL-terminated and outlives the call; the handle is
        // owned by `SingleInstance` and closed on drop.
        let handle = unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) }
            .context("CreateMutexW failed")?;
        // SAFETY: reads the calling thread's last error, set by the call above.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: the handle was just created and is not kept.
            unsafe { _ = CloseHandle(handle) };
            return Err(anyhow::Error::new(AlreadyRunning));
        }
        Ok(Self { handle })
    }

    /// Whether some process holds the mutex `name`, without taking it.
    ///
    /// `acquire` would answer the same question, but it creates the mutex
    /// when nobody holds it, and a watcher starting in that instant would
    /// read the probe as a running instance and exit. Opening an existing
    /// mutex creates nothing, and the handle is closed before returning so
    /// the object does not outlive the process that owns it.
    pub fn is_held(name: &str) -> bool {
        let name = HSTRING::from(format!("Local\\{name}"));
        // SAFETY: `name` is NUL-terminated and outlives the call; a handle
        // that comes back is closed here and kept nowhere.
        match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, PCWSTR(name.as_ptr())) } {
            Ok(handle) => {
                // SAFETY: the handle was just opened and is not used again.
                unsafe { _ = CloseHandle(handle) };
                true
            }
            Err(_) => false,
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY: the handle came from `CreateMutexW` and is closed once.
        unsafe { _ = CloseHandle(self.handle) };
    }
}
