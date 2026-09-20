//! Thin, safe wrappers around the few Win32 calls the program needs.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WAIT_OBJECT_0,
    WAIT_TIMEOUT, WPARAM,
};
use windows::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, INFINITE, OpenMutexW, ResetEvent, SYNCHRONIZATION_SYNCHRONIZE,
    SetEvent, WaitForMultipleObjects, WaitForSingleObject,
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

/// Why the watcher is stopping, which decides what a stop mid-game does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Nobody follows: *Quit*, `stop`, Ctrl-C, a logoff. The stop commands
    /// run, so the machine is not left on its gaming configuration.
    Restore,
    /// A watcher follows within seconds: an update, an upgrade, the
    /// development loop. The session stays open in the marker and the next
    /// watcher resumes it, so nothing runs twice. Decided 2026-09-18.
    Handover,
    /// The configuration file changed: the engine stops so that one built
    /// on the new file can take its place, in the same process. A session
    /// that is open stays open, as for a handover, and the next engine
    /// resumes it. Only ever the reason of a signal made with
    /// [`StopSignal::child_of`]. Decided 2026-09-19.
    Reload,
}

/// A manual-reset event used to unblock every wait in the program at once.
///
/// Waiting on a kernel event rather than checking a flag on a timer is what
/// keeps the watcher at zero wake-ups while a game is running.
///
/// A signal can be the child of another: it is then set when either event
/// is, and the parent's reason wins. That is how one engine run stops for
/// a reload without the process-wide stop ever being reset -- the child's
/// own event carries the reload and is reset between runs; the parent
/// carries *Quit*, the logoff and `stop`, and stays set once set.
pub struct StopSignal {
    event: HANDLE,
    /// Set before the event when the stop is a handover. The first reason to
    /// arrive wins: a *Quit* after a handover request still hands over, a
    /// handover after a *Quit* has nothing left to hand.
    handover: AtomicBool,
    /// Set before the event when the stop is a reload of this run only.
    reload: AtomicBool,
    /// The signal this one also answers to.
    parent: Option<Arc<StopSignal>>,
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
        Ok(Self {
            event,
            handover: AtomicBool::new(false),
            reload: AtomicBool::new(false),
            parent: None,
        })
    }

    /// A signal that is also set whenever `parent` is, and can be set on
    /// its own for a reload and reset again with [`take_reload`], leaving
    /// the parent as it was.
    ///
    /// [`take_reload`]: StopSignal::take_reload
    pub fn child_of(parent: &Arc<StopSignal>) -> Result<Self> {
        let mut child = Self::new()?;
        child.parent = Some(Arc::clone(parent));
        Ok(child)
    }

    /// Stop, and restore: the stop commands run if a game is on.
    pub fn signal(&self) {
        // SAFETY: the event is open for as long as `self` lives.
        let _ = unsafe { SetEvent(self.event) };
    }

    /// Stop, and hand an open session to the watcher that follows.
    pub fn signal_handover(&self) {
        if !self.is_set() {
            self.handover.store(true, Ordering::SeqCst);
        }
        self.signal();
    }

    /// Stop this run only, to start again on a changed configuration. Nothing
    /// to do when a stop is already under way: the process is leaving, and
    /// the file is read afresh at the next start.
    pub fn signal_reload(&self) {
        if self.is_set() {
            return;
        }
        self.reload.store(true, Ordering::SeqCst);
        self.signal();
    }

    /// Whether the stop under way is a reload, and if so, clear it so the
    /// next run waits afresh. A parent that is set is never cleared: the
    /// process is stopping, whatever this run was told.
    pub fn take_reload(&self) -> bool {
        if self.parent.as_ref().is_some_and(|parent| parent.is_set()) || !self.own_is_set() {
            return false;
        }
        if !self.reload.swap(false, Ordering::SeqCst) {
            return false;
        }
        // SAFETY: as for `signal`.
        let _ = unsafe { ResetEvent(self.event) };
        true
    }

    pub fn reason(&self) -> StopReason {
        if let Some(parent) = &self.parent
            && parent.is_set()
        {
            return parent.reason();
        }
        if self.handover.load(Ordering::SeqCst) {
            StopReason::Handover
        } else if self.reload.load(Ordering::SeqCst) {
            StopReason::Reload
        } else {
            StopReason::Restore
        }
    }

    pub fn is_set(&self) -> bool {
        self.own_is_set() || self.parent.as_ref().is_some_and(|parent| parent.is_set())
    }

    fn own_is_set(&self) -> bool {
        // SAFETY: as for `signal`.
        unsafe { WaitForSingleObject(self.event, 0) == WAIT_OBJECT_0 }
    }

    /// Park until the stop is signalled.
    pub fn wait(&self) {
        while !self.wait_timeout(Duration::MAX) {}
    }

    /// Wait up to `timeout`. Returns true when the stop was signalled, which
    /// callers treat as "give up and return".
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let millis = timeout.as_millis().min(u128::from(INFINITE - 1)) as u32;
        let handles = self.handles();
        // SAFETY: every handle is an event open for as long as `self` -- and
        // its parent, which it holds -- lives.
        let result = unsafe { WaitForMultipleObjects(&handles, false, millis) };
        result != WAIT_TIMEOUT && result.0 < WAIT_OBJECT_0.0 + handles.len() as u32
    }

    /// The events to wait on: this signal's own, and its parent's when it
    /// has one. For a wait that also watches something else.
    pub(crate) fn handles(&self) -> Vec<HANDLE> {
        let mut handles = vec![self.event];
        if let Some(parent) = &self.parent {
            handles.extend(parent.handles());
        }
        handles
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

/// Posted by another process of this program -- `stop --handover` -- where
/// `WM_CLOSE` would mean *Quit*. `WM_APP + 2` and `+ 3` belong to the tray.
const WM_HANDOVER: u32 = WM_APP + 4;

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
        // `WM_CLOSE` with a reason: the session is handed on, not closed.
        // Destroying the window is what the default procedure does for
        // `WM_CLOSE`, and it ends the message loop the same way.
        WM_HANDOVER => {
            if let Some(state) = SESSION.get() {
                tracing::debug!(
                    target: crate::logging::target::WATCHER,
                    "Asked to hand the session over, so the watcher stops without closing it"
                );
                state.stop.signal_handover();
            }
            // SAFETY: `window` is this procedure's own window, destroyed on
            // its own thread; `WM_DESTROY` follows and ends the loop.
            unsafe { _ = DestroyWindow(window) };
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

/// Ask a running watcher to stop: `WM_CLOSE` on its session window, the way
/// its *Quit* menu entry does, or `WM_HANDOVER` to leave an open session to
/// the watcher that follows. Either ends the message loop through
/// `WM_DESTROY`. Nothing here waits; the caller watches the single-instance
/// mutex to know the process is gone.
///
/// `FindWindowW` cannot see a class another process registered, so the
/// top-level windows are enumerated and asked their class name instead.
pub fn close_session_window(reason: StopReason) -> Result<()> {
    /// The message and the found flag, handed to the callback as one
    /// pointer.
    struct Visit {
        message: u32,
        found: bool,
    }
    unsafe extern "system" fn visit(window: HWND, visit: LPARAM) -> BOOL {
        let mut name = [0u16; 64];
        // SAFETY: `name` is a valid buffer and its length is what is passed;
        // GetClassNameW writes at most that many characters.
        let len = unsafe { GetClassNameW(window, &mut name) };
        if len > 0 && String::from_utf16_lossy(&name[..len as usize]) == SESSION_CLASS {
            // SAFETY: `visit` is the address of the caller's `Visit`, alive
            // for the whole enumeration and written only here.
            let visit = unsafe { &mut *(visit.0 as *mut Visit) };
            // SAFETY: the message carries no pointers; the window handle came
            // from the enumeration and may be gone by the time it is read,
            // which PostMessageW reports rather than dereferences.
            if unsafe { PostMessageW(Some(window), visit.message, WPARAM(0), LPARAM(0)) }.is_ok() {
                visit.found = true;
            }
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut state = Visit {
        message: match reason {
            StopReason::Restore => WM_CLOSE,
            StopReason::Handover => WM_HANDOVER,
            // A reload is the watcher's own business, between its engine
            // runs; nothing asks it of another process.
            StopReason::Reload => anyhow::bail!("a reload cannot be asked of a running watcher"),
        },
        found: false,
    };
    // SAFETY: the callback reads only what it is given and writes only to
    // `state`, whose address is passed and which outlives the call. An
    // enumeration the callback stops is reported as an error by EnumWindows,
    // which is why its result is not the verdict.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut state as *mut Visit as isize)) };
    anyhow::ensure!(state.found, "no running watcher was found");
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

// ---------------------------------------------------------------------------

/// What a wait on a [`FolderWatch`] came back with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderEvent {
    /// Something in the folder was written, renamed, created or removed.
    Changed,
    /// Nothing happened within the timeout.
    TimedOut,
    /// The stop signal was set.
    Stopped,
}

/// A change notification on one folder: a handle Windows signals when a
/// file in it is written, renamed, created or removed.
///
/// `FindFirstChangeNotificationW` rather than `ReadDirectoryChangesW`: the
/// program does not need to know *which* file changed, only that the folder
/// holding the configuration did, and a waitable handle is all that takes.
/// The watch does not descend into subfolders.
pub struct FolderWatch {
    handle: HANDLE,
}

// SAFETY: a change notification handle is a kernel object; waiting on it
// from the thread that watches, rather than the one that opened it, is what
// it is for, and the struct holds nothing else.
unsafe impl Send for FolderWatch {}

impl FolderWatch {
    pub fn open(dir: &std::path::Path) -> Result<Self> {
        let name = HSTRING::from(dir.as_os_str());
        // SAFETY: `name` is a NUL-terminated string that outlives the call;
        // the handle that comes back is owned by `FolderWatch` and closed
        // on drop.
        let handle = unsafe {
            FindFirstChangeNotificationW(
                &name,
                false,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_SIZE,
            )
        }
        .with_context(|| format!("cannot watch `{}` for changes", dir.display()))?;
        Ok(Self { handle })
    }

    /// Park until the folder changes, `stop` is set, or `timeout` passes.
    /// A change re-arms the handle before returning, so the next wait sees
    /// the next change.
    pub fn wait(&self, stop: &StopSignal, timeout: Option<Duration>) -> FolderEvent {
        let millis = match timeout {
            Some(timeout) => timeout.as_millis().min(u128::from(INFINITE - 1)) as u32,
            None => INFINITE,
        };
        let mut handles = vec![self.handle];
        handles.extend(stop.handles());
        // SAFETY: the notification handle lives as long as `self`, the
        // events as long as `stop`.
        let result = unsafe { WaitForMultipleObjects(&handles, false, millis) };
        if result == WAIT_OBJECT_0 {
            // SAFETY: re-arms the handle opened in `open`, still open.
            let _ = unsafe { FindNextChangeNotification(self.handle) };
            FolderEvent::Changed
        } else if result == WAIT_TIMEOUT {
            FolderEvent::TimedOut
        } else {
            FolderEvent::Stopped
        }
    }
}

impl Drop for FolderWatch {
    fn drop(&mut self) {
        // SAFETY: the handle came from `FindFirstChangeNotificationW` and is
        // closed once.
        unsafe { _ = FindCloseChangeNotification(self.handle) };
    }
}
