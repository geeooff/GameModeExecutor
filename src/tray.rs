//! The notification area icon and its menu.
//!
//! Hangs off the window Lot 5 created, which already exists for
//! `WM_QUERYENDSESSION` and already pumps messages on the main thread. The
//! window procedure in `win` hands anything it does not handle itself to
//! [`dispatch`].
//!
//! State lives in a thread local rather than a mutex: everything here runs on
//! the one thread that owns the window.
//!
//! **Nothing holds that borrow across a Win32 call that can pump messages**,
//! and the whole shape of this module comes from that rule. `TrackPopupMenuEx`
//! runs its own message loop while the menu is open, so the window procedure is
//! re-entered and `dispatch` is called again; a borrow held across it is a
//! second `borrow_mut` and a panic. The first version did exactly that and a
//! right-click killed the process. So every message is turned into a `Plan`
//! under a short borrow, and the plan is carried out with no borrow at all.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_NOSOUND,
    NIIF_RESPECT_QUIET_TIME, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NOTIFY_ICON_DATA_FLAGS, NOTIFY_ICON_INFOTIP_FLAGS, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
    Shell_NotifyIconW, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, DestroyIcon, DestroyMenu,
    GetSystemMetrics, HICON, IMAGE_FLAGS, LR_DEFAULTCOLOR, MF_DISABLED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, PostMessageW, RegisterWindowMessageW, SM_CXSMICON, SM_CYSMICON, SW_SHOWNORMAL,
    SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_APP,
    WM_CONTEXTMENU, WM_DPICHANGED, WM_NULL, WM_SETTINGCHANGE,
};
use windows::core::PCWSTR;

use crate::win::StopSignal;

/// Our callback message. `WM_APP + 1` is the watcher-finished message in `win`.
const WM_TRAY: u32 = WM_APP + 2;

/// Posted by the watcher thread when the session changed, or the
/// configuration became unusable or usable again.
const WM_SESSION: u32 = WM_APP + 3;

/// Posted by the updater's worker when an outcome left a notice to show.
/// `WM_APP + 4` is `win`'s handover message.
const WM_UPDATE: u32 = WM_APP + 5;

/// One wording for a game Windows flags but does not name, shared by the
/// tooltip and the menu and agreeing with what the log already says. Three
/// surfaces disagreeing about the same fact is worse than any of them being
/// terse.
const UNNAMED: &str = "A game is running, but Windows does not name it";

/// What the watcher is doing. The single source the icon, the tooltip and the
/// menu all read, so they cannot drift apart.
///
/// Shared across threads, unlike everything else in this module: the engine
/// runs on the worker and writes here, the window thread reads. A `Mutex` and a
/// posted message rather than sending the name through `PostMessage`, which has
/// nowhere to put a string that is not a raw pointer and a promise.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Session {
    Idle,
    /// `None` when Windows tracks the title but describes nothing.
    Playing(Option<String>),
}

static SESSION: std::sync::Mutex<Session> = std::sync::Mutex::new(Session::Idle);

/// Why the configuration cannot be used, in one line, or `None` while it
/// can. The second axis the icon is drawn from, written by the supervisor
/// in `service` through [`fault_sink`]. Nothing is watched while it is
/// `Some`, so it takes precedence over the session on every surface.
static FAULT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The two facts every surface is drawn from, read together so the icon,
/// the tooltip and the menu cannot disagree about either.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Facts {
    session: Session,
    fault: Option<String>,
}

fn facts() -> Facts {
    Facts {
        session: SESSION
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
        fault: FAULT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
    }
}

/// Hand this to the engine so it reports session changes here.
///
/// Returns early when nothing actually changed, which matters because the
/// engine reports on every refinement and most of those keep the same name.
pub fn session_sink(window: isize) -> crate::engine::SessionSink {
    Arc::new(move |session: &crate::engine::Session| {
        let next = match session {
            crate::engine::Session::Idle => Session::Idle,
            crate::engine::Session::Playing(signal) => {
                Session::Playing(signal.as_ref().and_then(|s| s.process_name.clone()))
            }
        };
        {
            let mut held = SESSION
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *held == next {
                return;
            }
            *held = next;
        }
        // Wake the thread that owns the window; it reads the value itself.
        // SAFETY: posting carries no pointers, and a window that is gone makes
        // the call fail, which is ignored.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(window as *mut std::ffi::c_void)),
                WM_SESSION,
                WPARAM(0),
                LPARAM(0),
            );
        }
    })
}

/// Hand this to the supervisor so it reports the configuration's faults
/// here: the summary is stored and the window's thread redraws from it.
pub fn fault_sink(window: isize) -> crate::config::FaultSink {
    Arc::new(move |fault: Option<&crate::config::LoadError>| {
        let next = fault.map(crate::config::LoadError::summary);
        {
            let mut held = FAULT
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *held == next {
                return;
            }
            *held = next;
        }
        // SAFETY: posting carries no pointers, and a window that is gone makes
        // the call fail, which is ignored.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(window as *mut std::ffi::c_void)),
                WM_SESSION,
                WPARAM(0),
                LPARAM(0),
            );
        }
    })
}

/// Hand this to the updater so it wakes the window's thread when an
/// outcome left a notice; the thread reads the notice itself.
pub fn update_sink(window: isize) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        // SAFETY: posting carries no pointers, and a window that is gone makes
        // the call fail, which is ignored.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(window as *mut std::ffi::c_void)),
                WM_UPDATE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    })
}

/// Dark context menus, through the only door Windows offers.
///
/// A menu built with `TrackPopupMenuEx` renders light whatever the taskbar is
/// set to, and **there is no documented way to change that**. What Explorer
/// does -- and wxWidgets, and every Win32 application with a dark menu,
/// including the Bluetooth icon two slots along in the same tray -- is call
/// `SetPreferredAppMode` in `uxtheme.dll`. It is undocumented, exported by
/// ordinal only, and not exported by name at all on Windows 11.
///
/// This project has turned down workarounds before: MSIX for virtualising
/// `%APPDATA%`, `AttachConsole` for losing exit codes. Those failed *silently*
/// and wrongly. This one fails visibly and harmlessly: if the ordinal moves or
/// the call disappears, the menu is light again and nothing else changes. That
/// difference is the whole reason it is here and they are not.
///
/// Every step is guarded. An old Windows, a missing export, a library that will
/// not load -- each simply means no call and a light menu.
mod dark {
    use std::sync::OnceLock;

    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::{PCSTR, PCWSTR};

    /// `SetPreferredAppMode` arrived in Windows 10 1903. On 1809 the same
    /// ordinal is `AllowDarkModeForApp`, which takes a `BOOL` rather than an
    /// enum -- calling one thinking it is the other is the kind of mistake a
    /// version check is for.
    const FIRST_BUILD_WITH_PREFERRED_APP_MODE: u32 = 18362;

    /// `PreferredAppMode::AllowDark`: follow the system rather than force dark.
    const ALLOW_DARK: i32 = 1;

    const ORDINAL_SET_PREFERRED_APP_MODE: usize = 135;
    const ORDINAL_FLUSH_MENU_THEMES: usize = 136;

    type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
    type FlushMenuThemes = unsafe extern "system" fn();

    static FLUSH: OnceLock<Option<FlushMenuThemes>> = OnceLock::new();

    /// Ask for dark menus once, at startup. Safe to call when it cannot work.
    pub fn enable() {
        let Some(library) = uxtheme() else {
            return;
        };
        if build() < FIRST_BUILD_WITH_PREFERRED_APP_MODE {
            tracing::debug!(
                target: crate::logging::target::WATCHER,
                build = build(),
                "This Windows predates dark menus for Win32 applications"
            );
            return;
        }

        let Some(set) = resolve::<SetPreferredAppMode>(library, ORDINAL_SET_PREFERRED_APP_MODE)
        else {
            tracing::debug!(
                target: crate::logging::target::WATCHER,
                "uxtheme has no SetPreferredAppMode; the menu stays light"
            );
            return;
        };
        // SAFETY: the pointer was resolved from uxtheme by ordinal, on a build
        // where that ordinal is documented by observation to take one
        // `PreferredAppMode` argument; the signature was checked against that
        // build number before this point.
        unsafe { set(ALLOW_DARK) };
        flush();
        tracing::debug!(
            target: crate::logging::target::WATCHER,
            "Menus follow the system theme"
        );
    }

    /// Windows caches menu themes; after a theme change the cache is stale.
    pub fn flush() {
        let flush = FLUSH.get_or_init(|| {
            uxtheme()
                .and_then(|library| resolve::<FlushMenuThemes>(library, ORDINAL_FLUSH_MENU_THEMES))
        });
        if let Some(flush) = flush {
            // SAFETY: as for `SetPreferredAppMode`: a no-argument export
            // resolved by ordinal on a checked build.
            unsafe { flush() };
        }
    }

    fn uxtheme() -> Option<HMODULE> {
        static LIBRARY: OnceLock<Option<isize>> = OnceLock::new();
        let handle = LIBRARY.get_or_init(|| {
            let name: Vec<u16> = "uxtheme.dll\0".encode_utf16().collect();
            // SAFETY: `name` is NUL-terminated and outlives the call. The
            // module is never freed: it is a system DLL kept for the process's
            // lifetime.
            unsafe { LoadLibraryW(PCWSTR(name.as_ptr())) }
                .ok()
                .map(|module| module.0 as isize)
        });
        handle.map(|handle| HMODULE(handle as *mut std::ffi::c_void))
    }

    /// `GetProcAddress` takes an ordinal as a pointer whose value *is* the
    /// number, which is what `MAKEINTRESOURCE` means in C.
    fn resolve<T>(library: HMODULE, ordinal: usize) -> Option<T> {
        // SAFETY: an ordinal is passed as a pointer whose value is the number,
        // which is what the API documents; `library` was loaded above.
        let address = unsafe { GetProcAddress(library, PCSTR(ordinal as *const u8)) }?;
        // SAFETY: a function pointer is reinterpreted as another function
        // pointer type of the same size. Whether the signature is right is the
        // caller's claim, checked by the ordinal and the build number and by
        // nothing else. That is the bargain, and it is why this module lets
        // exactly two ordinals in.
        Some(unsafe { std::mem::transmute_copy::<_, T>(&address) })
    }

    /// From the registry rather than `GetVersionEx`, which lies about anything
    /// past Windows 8 unless the executable carries a compatibility manifest.
    fn build() -> u32 {
        crate::registry::Key::open_local_machine(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .ok()
            .and_then(|key| key.string_value("CurrentBuildNumber"))
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    }
}

/// What the icon should be showing, derived rather than stored.
fn current_state() -> State {
    state_for(&facts())
}

/// A fault is the error state whatever the session: nothing is watched
/// while the configuration is unusable, so a green icon would be a lie.
fn state_for(facts: &Facts) -> State {
    if facts.fault.is_some() {
        return State::Error;
    }
    match facts.session {
        Session::Idle => State::Idle,
        Session::Playing(_) => State::Active,
    }
}

/// Windows truncates `szTip` at 128 units including the terminator, and a
/// game's name is not always short.
fn tooltip() -> String {
    truncate(&tooltip_for(&facts()), 127)
}

fn tooltip_for(facts: &Facts) -> String {
    if facts.fault.is_some() {
        return "GameModeExecutor - configuration error".to_owned();
    }
    match &facts.session {
        Session::Idle => "GameModeExecutor - no game detected".to_owned(),
        Session::Playing(Some(name)) => format!("GameModeExecutor - playing {name}"),
        Session::Playing(None) => format!("GameModeExecutor - {UNNAMED}"),
    }
}

/// The disabled first line of the menu: the same fact, room for more words
/// -- and, for a fault, the words that say what to fix, next to the *Edit
/// configuration* entry that opens the file. Cut where a menu would run
/// off the screen; the log has the whole line.
fn menu_header() -> String {
    truncate(&menu_header_for(&facts()), 160)
}

fn menu_header_for(facts: &Facts) -> String {
    if let Some(fault) = &facts.fault {
        return format!("Configuration error: {fault}");
    }
    match &facts.session {
        Session::Idle => "No game detected".to_owned(),
        Session::Playing(Some(name)) => format!("Playing {name}"),
        Session::Playing(None) => UNNAMED.to_owned(),
    }
}

/// Cut to `limit` UTF-16 units without splitting a character.
fn truncate(text: &str, limit: usize) -> String {
    if text.encode_utf16().count() <= limit {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut units = 0;
    for character in text.chars() {
        let width = character.len_utf16();
        if units + width > limit - 1 {
            break;
        }
        out.push(character);
        units += width;
    }
    out.push('\u{2026}');
    out
}

const ID_CONFIG: usize = 1;
const ID_LOG: usize = 2;
const ID_DOCS: usize = 3;
const ID_QUIT: usize = 4;
/// The update section's entries, one id per item in the order `update`
/// lists them. The section is whatever `update::view()` says: this module
/// draws it and holds no rule about it.
const ID_UPDATE_BASE: usize = 100;

/// One icon per state and taskbar theme, compiled in.
///
/// Embedded rather than loaded from disk so a hand-installed copy is one folder
/// with nothing to lose. It costs about 190 KB across the six, which is the price of
/// never having to find a file at runtime.
const ICONS: [(State, Theme, &[u8]); 6] = [
    (
        State::Idle,
        Theme::Dark,
        include_bytes!("../assets/icons/gamemode-idle-dark.ico"),
    ),
    (
        State::Idle,
        Theme::Light,
        include_bytes!("../assets/icons/gamemode-idle-light.ico"),
    ),
    (
        State::Active,
        Theme::Dark,
        include_bytes!("../assets/icons/gamemode-active-dark.ico"),
    ),
    (
        State::Active,
        Theme::Light,
        include_bytes!("../assets/icons/gamemode-active-light.ico"),
    ),
    (
        State::Error,
        Theme::Dark,
        include_bytes!("../assets/icons/gamemode-error-dark.ico"),
    ),
    (
        State::Error,
        Theme::Light,
        include_bytes!("../assets/icons/gamemode-error-light.ico"),
    ),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// Running, no game. What the program shows almost all the time.
    Idle,
    /// A game is detected.
    Active,
    /// The configuration cannot be used and nothing is watched until it is
    /// fixed; the menu's first line says what is wrong. The one standing
    /// error the program has, since 2026-09-19; the artwork was reserved
    /// for it so nobody reached for the slash to mean something else.
    Error,
}

/// Which taskbar the icon has to be legible on -- not the colour of the
/// drawing. `Dark` is the *brighter* artwork, because it sits on a dark bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Dark,
    Light,
}

impl Theme {
    /// `SystemUsesLightTheme` governs the taskbar; `AppsUseLightTheme` governs
    /// application windows, and the two can differ. Reading the wrong one
    /// leaves the icon invisible on some machines rather than merely wrong.
    fn current() -> Self {
        let light = crate::registry::Key::open_current_user(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        )
        .ok()
        .and_then(|key| key.dword_value("SystemUsesLightTheme"))
        .unwrap_or(0);
        if light == 0 {
            Theme::Dark
        } else {
            Theme::Light
        }
    }
}

/// What the menu opens.
pub struct Targets {
    pub config: PathBuf,
    pub log: PathBuf,
}

struct Tray {
    window: HWND,
    icon: HICON,
    /// Exactly what the shell is showing right now: the icon's state and theme,
    /// and the tooltip text. The *truth* is [`SESSION`] and the taskbar theme;
    /// this is what was last drawn from them, so a refresh can tell whether
    /// there is anything to do and stay quiet when there is not.
    shown: (State, Theme, String),
    targets: Targets,
    stop: Arc<StopSignal>,
    /// Broadcast by the shell when Explorer restarts. Every icon is lost then,
    /// and an application that does not listen for this loses its own for the
    /// rest of the session -- the classic version of this bug.
    taskbar_created: u32,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

/// What `dispatch` does once it has let go of the borrow.
///
/// Every variant below this point is carried out with nothing borrowed, which
/// is what makes re-entrancy harmless.
enum Plan {
    /// Not ours; let Windows have it.
    Ignore,
    /// Ours, nothing more to do.
    Handled,
    ShowMenu(HWND, POINT),
    /// The theme or the scaling moved.
    Reload,
    /// Explorer restarted and took the icon with it.
    ReAdd,
    /// The updater has something to say; the notice is read with no borrow.
    Notify,
}

/// Add the icon. Call once, from the thread owning `window`.
pub fn install(window: isize, targets: Targets, stop: Arc<StopSignal>) -> Result<()> {
    let window = HWND(window as *mut std::ffi::c_void);
    let theme = Theme::current();
    let icon =
        load_icon(State::Idle, theme, window).context("cannot build the notification icon")?;
    let name = wide("TaskbarCreated");
    // SAFETY: `name` is NUL-terminated and outlives the call.
    let taskbar_created = unsafe { RegisterWindowMessageW(PCWSTR(name.as_ptr())) };

    // Before the first menu is ever built.
    dark::enable();

    let tray = Tray {
        window,
        icon,
        shown: (State::Idle, theme, tooltip()),
        targets,
        stop,
        taskbar_created,
    };
    let data = tray.data();
    TRAY.with(|cell| *cell.borrow_mut() = Some(tray));
    add(&data)?;

    tracing::debug!(
        target: crate::logging::target::WATCHER,
        theme = ?theme,
        size = small_icon_size(window).0,
        "Notification icon added"
    );
    Ok(())
}

/// Remove the icon. The shell keeps a ghost otherwise, until something hovers
/// over it.
pub fn uninstall() {
    let Some((data, icon)) = TRAY.with(|cell| {
        cell.borrow_mut()
            .take()
            .map(|tray| (tray.data(), tray.icon))
    }) else {
        return;
    };
    // SAFETY: `data` is the fully initialised struct the icon was added with,
    // and `icon` is the handle this module created, destroyed once here.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        let _ = DestroyIcon(icon);
    }
}

/// Messages `win`'s window procedure did not handle. Returns `Some` when this
/// module dealt with one.
pub fn dispatch(message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    // Short borrow: a decision, and nothing that can re-enter.
    let plan = TRAY.with(|cell| {
        cell.borrow()
            .as_ref()
            .map_or(Plan::Ignore, |tray| tray.plan(message, wparam, lparam))
    });

    match plan {
        Plan::Ignore => None,
        Plan::Handled => Some(LRESULT(0)),
        Plan::ShowMenu(window, at) => {
            show_menu(window, at);
            Some(LRESULT(0))
        }
        Plan::Reload => {
            reload();
            // Let Windows see these two as well: other things listen for them.
            None
        }
        Plan::ReAdd => {
            re_add();
            Some(LRESULT(0))
        }
        Plan::Notify => {
            if let Some(notice) = crate::update::take_notice() {
                notify(&notice.title, &notice.text);
            }
            Some(LRESULT(0))
        }
    }
}

impl Tray {
    /// Decide, touching nothing outside this struct.
    fn plan(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> Plan {
        if message == self.taskbar_created {
            return Plan::ReAdd;
        }
        match message {
            WM_TRAY => {
                // With NOTIFYICON_VERSION_4 the event is in the low word of
                // lParam and the cursor position is in wParam, which is why the
                // version is set at all: the old packing had no room for both.
                if (lparam.0 as u32) & 0xFFFF == WM_CONTEXTMENU {
                    let x = i32::from((wparam.0 & 0xFFFF) as i16);
                    let y = i32::from(((wparam.0 >> 16) & 0xFFFF) as i16);
                    Plan::ShowMenu(self.window, POINT { x, y })
                } else {
                    Plan::Handled
                }
            }
            WM_SETTINGCHANGE if setting_is(lparam, "ImmersiveColorSet") => Plan::Reload,
            WM_DPICHANGED => Plan::Reload,
            // The engine says a game started, was renamed, or ended; or the
            // supervisor says the configuration broke or was fixed.
            WM_SESSION => Plan::Reload,
            WM_UPDATE => Plan::Notify,
            _ => Plan::Ignore,
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.window,
            uID: 1,
            uFlags: NOTIFY_ICON_DATA_FLAGS(NIF_ICON.0 | NIF_MESSAGE.0 | NIF_TIP.0 | NIF_SHOWTIP.0),
            uCallbackMessage: WM_TRAY,
            hIcon: self.icon,
            ..Default::default()
        };
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let tip = wide(&tooltip());
        let len = tip.len().min(data.szTip.len());
        data.szTip[..len].copy_from_slice(&tip[..len]);
        data
    }
}

// ---------------------------------------------------------------------------
// Everything below runs with no borrow held.
// ---------------------------------------------------------------------------

/// A notification from the icon: the answer to something the user clicked,
/// since the menu they clicked in closed under them as every menu does.
/// Silent, and held back during quiet hours; Windows shows it as a toast
/// and keeps it in the notification centre. Never for anything the user
/// did not ask for.
fn notify(title: &str, text: &str) {
    let Some(mut data) = TRAY.with(|cell| cell.borrow().as_ref().map(Tray::data)) else {
        return;
    };
    data.uFlags = NOTIFY_ICON_DATA_FLAGS(data.uFlags.0 | NIF_INFO.0);
    data.dwInfoFlags =
        NOTIFY_ICON_INFOTIP_FLAGS(NIIF_INFO.0 | NIIF_NOSOUND.0 | NIIF_RESPECT_QUIET_TIME.0);
    let title_w = wide(title);
    let len = title_w.len().min(data.szInfoTitle.len() - 1);
    data.szInfoTitle[..len].copy_from_slice(&title_w[..len]);
    let text_w = wide(text);
    let len = text_w.len().min(data.szInfo.len() - 1);
    data.szInfo[..len].copy_from_slice(&text_w[..len]);
    // SAFETY: `data` is the fully initialised struct the icon was added with,
    // its strings NUL-terminated within their buffers; nothing in the tray is
    // borrowed while the shell handles it.
    if unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool() {
        tracing::debug!(
            target: crate::logging::target::UPDATE,
            title,
            "Notification shown"
        );
    }
}

fn add(data: &NOTIFYICONDATAW) -> Result<()> {
    // SAFETY: `data` is fully initialised, `cbSize` included, and the handles
    // it carries are live.
    unsafe {
        Shell_NotifyIconW(NIM_ADD, data)
            .ok()
            .context("Shell_NotifyIcon could not add the icon")?;
        // Opt into the version 4 behaviour. Without this the callback arrives
        // in the old packing and the coordinates are wrong.
        let _ = Shell_NotifyIconW(NIM_SETVERSION, data);
    }
    Ok(())
}

fn re_add() {
    let Some(data) = TRAY.with(|cell| cell.borrow().as_ref().map(Tray::data)) else {
        return;
    };
    if add(&data).is_ok() {
        tracing::debug!(
            target: crate::logging::target::WATCHER,
            "Explorer restarted, notification icon added again"
        );
    }
}

/// Re-read the theme and rebuild the icon at the size Windows wants now.
fn reload() {
    let Some((window, shown)) = TRAY.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|tray| (tray.window, tray.shown.clone()))
    }) else {
        return;
    };

    // Read the truth afresh. The theme is read here rather than trusted from
    // the message that woke us, because we are not always woken: a tool that
    // switches the theme on a schedule may write the registry without
    // broadcasting, and this is also called when a menu is about to open.
    let wanted = (current_state(), Theme::current(), tooltip());
    if wanted == shown {
        return;
    }

    let rebuild = wanted.0 != shown.0 || wanted.1 != shown.1;
    if wanted.1 != shown.1 {
        // Windows caches menu themes, so a menu built after a theme change
        // would keep the old one until something invalidates it.
        dark::flush();
    }
    let fresh = rebuild
        .then(|| load_icon(wanted.0, wanted.1, window).ok())
        .flatten();

    // Swap under a short borrow, then talk to the shell outside it.
    let swapped = TRAY.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let tray = borrowed.as_mut()?;
        let previous = fresh.map(|icon| std::mem::replace(&mut tray.icon, icon));
        tray.shown = wanted.clone();
        Some((previous, tray.data()))
    });
    let Some((previous, data)) = swapped else {
        if let Some(icon) = fresh {
            // SAFETY: an icon this function just created and will not use.
            unsafe { _ = DestroyIcon(icon) };
        }
        return;
    };

    // SAFETY: `data` is the fully initialised struct with the new icon; the
    // previous icon is no longer referenced by the shell once the modify call
    // returns, and is destroyed once.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        if let Some(previous) = previous {
            let _ = DestroyIcon(previous);
        }
    }
    tracing::debug!(
        target: crate::logging::target::WATCHER,
        state = ?wanted.0,
        theme = ?wanted.1,
        tooltip = %wanted.2,
        "Notification icon refreshed"
    );
}

/// Show the context menu and act on what was chosen.
///
/// `TPM_RETURNCMD` with `TPM_NONOTIFY` is deliberate: the alternative posts
/// `WM_COMMAND` to the window *while the menu's own message loop is still
/// running*, which is a second re-entrant path into `dispatch`. Returning the
/// id instead means the command is handled after the menu has closed, here,
/// with nothing borrowed.
fn show_menu(window: HWND, at: POINT) {
    // The last chance to be right, and the one that does not depend on having
    // been told. A theme can move without `WM_SETTINGCHANGE` reaching us -- a
    // scheduler that writes the registry and broadcasts nothing, a message lost
    // while something else held the loop -- and the menu about to be built
    // would carry the old one. Re-reading here costs a registry read per
    // right-click and removes the whole class of problem. It is a no-op when
    // nothing moved.
    reload();

    // SAFETY: no arguments; the menu is destroyed below on every path.
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };

    // Kept alive until after TrackPopupMenuEx returns.
    let header = wide(&menu_header());
    let config = wide("Edit configuration");
    let log = wide("Open log");
    let docs = wide("Documentation");
    let quit = wide("Quit");
    // The update section, as the object renders it right now. Read once,
    // before the menu is built, and used again after it closes to know what
    // an id meant -- the phase may have moved meanwhile, and a stale click
    // is one the object ignores.
    let updates = crate::update::view();
    let update_labels: Vec<Vec<u16>> = updates.iter().map(|item| wide(&item.label)).collect();

    // SAFETY: the strings outlive the block, `menu` was just created and is
    // destroyed at the end, and `window` is the tray's own window. Nothing
    // in the tray is borrowed across this block: `TrackPopupMenuEx` pumps
    // messages and re-enters the window procedure.
    let chosen = unsafe {
        // Disabled on purpose: it is the answer to "what is running", not
        // something to click. Id 0 so a stray selection means nothing.
        let _ = AppendMenuW(
            menu,
            MF_STRING | MF_DISABLED | MF_GRAYED,
            0,
            PCWSTR(header.as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_CONFIG, PCWSTR(config.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, ID_LOG, PCWSTR(log.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, ID_DOCS, PCWSTR(docs.as_ptr()));
        if !updates.is_empty() {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            for (i, (item, label)) in updates.iter().zip(&update_labels).enumerate() {
                // A disabled entry carries a sentence and cannot be chosen;
                // id 0 so a stray selection means nothing.
                let (flags, id) = if item.enabled && item.action.is_some() {
                    (MF_STRING, ID_UPDATE_BASE + i)
                } else {
                    (MF_STRING | MF_DISABLED | MF_GRAYED, 0)
                };
                let _ = AppendMenuW(menu, flags, id, PCWSTR(label.as_ptr()));
            }
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, PCWSTR(quit.as_ptr()));

        // Documented requirement: without it the menu stays on screen when the
        // user clicks elsewhere, because the owner window is not foreground and
        // never learns it lost the click.
        let _ = SetForegroundWindow(window);
        let chosen = TrackPopupMenuEx(
            menu,
            TPM_RIGHTBUTTON.0 | TPM_RETURNCMD.0 | TPM_NONOTIFY.0,
            at.x,
            at.y,
            window,
            None,
        );
        // The other half of the same workaround.
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        chosen.0 as usize
    };

    if let Some(action) = chosen
        .checked_sub(ID_UPDATE_BASE)
        .and_then(|i| updates.get(i))
        .and_then(|item| item.action.clone())
    {
        run_update_action(action);
        return;
    }
    run_command(chosen);
}

/// What an update entry asked for. The page opens here, because the shell
/// is this module's business; everything else is the object's.
fn run_update_action(action: crate::update::Action) {
    match action {
        crate::update::Action::OpenReleasePage(url) => {
            tracing::debug!(
                target: crate::logging::target::UPDATE,
                url,
                "Opening the release page"
            );
            open(&url, None);
        }
        other => crate::update::perform(other),
    }
}

fn run_command(id: usize) {
    match id {
        ID_CONFIG | ID_LOG => {
            // Copy the path out, then let go: ShellExecuteW can show UI of its
            // own, which pumps messages like anything else.
            let path = TRAY.with(|cell| {
                cell.borrow().as_ref().map(|tray| {
                    if id == ID_CONFIG {
                        tray.targets.config.clone()
                    } else {
                        tray.targets.log.clone()
                    }
                })
            });
            if let Some(path) = path {
                open_path(&path);
            }
        }
        ID_DOCS => {
            open(crate::build_info::DOCS_URL, None);
        }
        ID_QUIT => {
            let stop = TRAY.with(|cell| cell.borrow().as_ref().map(|tray| Arc::clone(&tray.stop)));
            if let Some(stop) = stop {
                tracing::info!(
                    target: crate::logging::target::WATCHER,
                    "Quit chosen from the notification icon"
                );
                // Same path as Ctrl-C and as logging off: the engine unwinds,
                // the stop commands run, the message loop ends on its own.
                stop.signal();
            }
        }
        _ => {}
    }
}

/// Open a file the way the user's own settings say to, falling back to Notepad.
fn open_path(path: &std::path::Path) {
    let target = path.to_string_lossy().into_owned();
    if open(&target, None) {
        return;
    }
    // A `.toml` with no association is the likely miss, and a menu entry that
    // silently does nothing is worse than one that opens a plain editor.
    tracing::debug!(
        target: crate::logging::target::WATCHER,
        path = %path.display(),
        "No association for this file, opening it in Notepad"
    );
    open("notepad.exe", Some(&format!("\"{target}\"")));
}

/// Returns false when the shell refused. `ShellExecuteW` hands back a fake
/// `HINSTANCE` whose value is an error code at or below 32.
fn open(target: &str, arguments: Option<&str>) -> bool {
    let verb = wide("open");
    let target = wide(target);
    let arguments = arguments.map(wide);
    // SAFETY: every string is NUL-terminated and outlives the call, and
    // nothing in the tray is borrowed while the shell may show UI.
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            arguments
                .as_ref()
                .map_or(PCWSTR::null(), |a| PCWSTR(a.as_ptr())),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    result.0 as usize > 32
}

/// Build an `HICON` at the size the shell asks for, from the compiled-in file.
fn load_icon(state: State, theme: Theme, window: HWND) -> Result<HICON> {
    let data = ICONS
        .iter()
        .find(|(s, t, _)| *s == state && *t == theme)
        .map(|(_, _, data)| *data)
        .context("no icon for this state and theme")?;

    let (cx, cy) = small_icon_size(window);
    let frame = best_frame(data, cx).context("the icon file has no usable frame")?;

    // 0x0003_0000 is the icon format version, and the only value Windows
    // accepts here.
    // SAFETY: `frame` is a slice inside a compiled-in `.ico`, so it lives for
    // the whole program; the length passed is the slice's own.
    unsafe {
        CreateIconFromResourceEx(
            frame,
            true,
            0x0003_0000,
            cx,
            cy,
            IMAGE_FLAGS(LR_DEFAULTCOLOR.0),
        )
    }
    .context("CreateIconFromResourceEx failed")
}

/// The size the shell wants a notification icon to be, in real pixels.
///
/// `GetSystemMetrics` alone answers for 96 dpi and would say 16 on a display
/// that wants 24. `GetSystemMetricsForDpi` with the window's own dpi is the
/// pair that tells the truth, and it needs the process to have declared itself
/// dpi-aware first -- see `win::declare_dpi_awareness`.
fn small_icon_size(window: HWND) -> (i32, i32) {
    // SAFETY: a handle query with no pointers; an invalid handle yields 0.
    let dpi = unsafe { GetDpiForWindow(window) };
    if dpi == 0 {
        // No window yet, or an older Windows. The unscaled values are still
        // better than nothing.
        // SAFETY: plain integer queries.
        return unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };
    }
    // SAFETY: plain integer queries.
    unsafe {
        (
            GetSystemMetricsForDpi(SM_CXSMICON, dpi),
            GetSystemMetricsForDpi(SM_CYSMICON, dpi),
        )
    }
}

/// The frame of an `.ico` file closest to `wanted` pixels.
///
/// `LookupIconIdFromDirectoryEx` is the API for this and cannot be used: it
/// expects `RT_GROUP_ICON` resource data, whose entries hold resource ids,
/// where an `.ico` *file*'s entries hold byte offsets. One header, two layouts.
fn best_frame(data: &[u8], wanted: i32) -> Option<&[u8]> {
    let count = u16::from_le_bytes([*data.get(4)?, *data.get(5)?]) as usize;
    let mut best: Option<(u32, usize, usize)> = None;
    for index in 0..count {
        let at = 6 + index * 16;
        let entry = data.get(at..at + 16)?;
        let width = if entry[0] == 0 {
            256
        } else {
            i32::from(entry[0])
        };
        let length = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
        let score = frame_score(width, wanted);
        if best.is_none_or(|(current, _, _)| score < current) {
            best = Some((score, offset, length));
        }
    }
    let (_, offset, length) = best?;
    data.get(offset..offset.checked_add(length)?)
}

/// Lower is better. An exact match wins; otherwise a frame larger than asked
/// for beats a smaller one, because downscaling keeps detail that upscaling
/// invents.
fn frame_score(width: i32, wanted: i32) -> u32 {
    match width.cmp(&wanted) {
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => (width - wanted) as u32,
        std::cmp::Ordering::Less => 10_000 + (wanted - width) as u32,
    }
}

/// True when a `WM_SETTINGCHANGE` names the given section.
fn setting_is(lparam: LPARAM, name: &str) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    let pointer = lparam.0 as *const u16;
    let mut units = Vec::new();
    // The string is short and null-terminated; the cap is there so a stray
    // pointer cannot walk memory forever.
    for offset in 0..64 {
        // SAFETY: Windows hands `WM_SETTINGCHANGE` a pointer to a
        // NUL-terminated string that is valid for the duration of the message;
        // it was checked non-null above, and the read stops at the terminator
        // or at 64 units, whichever comes first.
        let unit = unsafe { *pointer.add(offset) };
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    String::from_utf16_lossy(&units) == name
}

/// A null-terminated UTF-16 buffer. The caller keeps it alive for as long as
/// the pointer is in use, which is why nothing here leaks one.
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_and_theme_has_an_icon() {
        for state in [State::Idle, State::Active, State::Error] {
            for theme in [Theme::Dark, Theme::Light] {
                assert!(
                    ICONS.iter().any(|(s, t, _)| *s == state && *t == theme),
                    "{state:?}/{theme:?} has no icon"
                );
            }
        }
    }

    #[test]
    fn the_compiled_in_icons_are_real_ico_files() {
        for (state, theme, data) in ICONS {
            // Reserved word, then type 1 for an icon.
            assert_eq!(&data[0..4], &[0, 0, 1, 0], "{state:?}/{theme:?}");
            let count = u16::from_le_bytes([data[4], data[5]]);
            assert_eq!(count, 8, "{state:?}/{theme:?} should carry eight frames");
        }
    }

    #[test]
    fn a_frame_is_found_for_every_size_windows_asks_for() {
        // 16 at 100%, 20 at 125%, 24 at 150%, 32 at 200%, 48 at 300%.
        for wanted in [16, 20, 24, 32, 40, 48, 64] {
            for (state, theme, data) in ICONS {
                let frame = best_frame(data, wanted);
                assert!(frame.is_some(), "{state:?}/{theme:?} at {wanted}");
                // Every frame in these files is PNG-compressed.
                assert_eq!(
                    &frame.unwrap()[1..4],
                    b"PNG",
                    "{state:?}/{theme:?} at {wanted}"
                );
            }
        }
    }

    #[test]
    fn an_exact_size_beats_anything_else() {
        assert_eq!(frame_score(24, 24), 0);
        // Bigger is preferred to smaller: 32 for a wanted 24 beats 20.
        assert!(frame_score(32, 24) < frame_score(20, 24));
        // And among bigger ones, the closest wins.
        assert!(frame_score(32, 24) < frame_score(256, 24));
    }

    /// The icon, the tooltip and the menu answer one question, so they are
    /// checked against one another rather than one at a time.
    #[test]
    fn the_three_surfaces_agree() {
        let playing = sound(Session::Playing(Some("bf6.exe".to_owned())));
        assert_eq!(state_for(&playing), State::Active);
        assert!(tooltip_for(&playing).contains("bf6.exe"));
        assert!(menu_header_for(&playing).contains("bf6.exe"));

        let idle = sound(Session::Idle);
        assert_eq!(state_for(&idle), State::Idle);
        assert!(tooltip_for(&idle).contains("no game"));
        assert!(menu_header_for(&idle).contains("No game"));
    }

    /// A configuration that cannot be used is the error state on every
    /// surface, whatever the session: nothing is watched meanwhile. The
    /// menu carries the reason, next to the entry that opens the file.
    #[test]
    fn a_configuration_fault_overrides_the_session_on_every_surface() {
        for session in [Session::Idle, Session::Playing(Some("bf6.exe".to_owned()))] {
            let faulty = Facts {
                session,
                fault: Some("line 3: unknown field `log_levl`".to_owned()),
            };
            assert_eq!(state_for(&faulty), State::Error);
            assert_eq!(
                tooltip_for(&faulty),
                "GameModeExecutor - configuration error"
            );
            assert_eq!(
                menu_header_for(&faulty),
                "Configuration error: line 3: unknown field `log_levl`"
            );
        }
    }

    /// A title Windows tracks but does not describe. All three have to say
    /// something, and the same something -- an empty space would read as a bug.
    #[test]
    fn an_unnamed_game_still_reads_sensibly() {
        let unnamed = sound(Session::Playing(None));
        assert_eq!(state_for(&unnamed), State::Active);
        assert_eq!(menu_header_for(&unnamed), UNNAMED);
        assert!(tooltip_for(&unnamed).contains(UNNAMED));
    }

    /// `szTip` holds 128 units including the terminator, and a game's name is
    /// not always short. Windows truncates silently, so we do it visibly.
    #[test]
    fn a_very_long_name_is_cut_to_fit() {
        let long = sound(Session::Playing(Some("x".repeat(400))));
        let text = truncate(&tooltip_for(&long), 127);
        assert!(text.encode_utf16().count() <= 127, "{}", text.len());
        assert!(text.ends_with('\u{2026}'), "{text}");
    }

    /// Cutting must not split a character in half.
    #[test]
    fn truncation_keeps_characters_whole() {
        let text = truncate(&"é".repeat(50), 10);
        assert!(text.encode_utf16().count() <= 10);
        assert!(text.chars().all(|c| c == 'é' || c == '\u{2026}'), "{text}");
    }

    /// A session with no configuration fault.
    fn sound(session: Session) -> Facts {
        Facts {
            session,
            fault: None,
        }
    }

    /// The two sinks write process-wide state, so the tests that drive them
    /// take turns.
    static SURFACES: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The fault sink stores the summary, the surfaces switch to the error
    /// state over whatever the session is, and `None` gives them back.
    /// Window `0`, as below.
    #[test]
    fn the_fault_sink_overlays_the_session_and_lifts_again() {
        let _turn = SURFACES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = std::path::Path::new("config.toml");
        let fault = crate::config::Config::parse("[general]\nlog_levl = 1\n", path).unwrap_err();
        let sink = fault_sink(0);

        sink(Some(&fault));
        assert_eq!(current_state(), State::Error);
        assert!(tooltip().contains("configuration error"));
        assert!(menu_header().starts_with("Configuration error: line 2: unknown field"));

        sink(None);
        assert_eq!(current_state(), State::Idle);
        assert!(tooltip().contains("no game"));
    }

    /// The engine reports on every refinement, and most of those land on the
    /// same name. Without the early return the shell would be asked to redraw
    /// an identical icon each time.
    ///
    /// Window `0` is deliberate: `PostMessageW` fails harmlessly on it, which
    /// is what lets the plumbing be tested without a window.
    #[test]
    fn the_sink_carries_changes_and_swallows_repeats() {
        let _turn = SURFACES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sink = session_sink(0);
        let signal = crate::detect::GameSignal {
            source: "test",
            process_name: Some("bf6.exe".to_owned()),
            process_id: Some(14552),
            process_path: None,
        };

        use crate::engine::Session as Engine;

        sink(&Engine::Playing(Some(signal.clone())));
        assert_eq!(
            facts().session,
            Session::Playing(Some("bf6.exe".to_owned()))
        );
        assert_eq!(current_state(), State::Active);
        assert!(tooltip().contains("bf6.exe"));

        // The same thing again changes nothing.
        sink(&Engine::Playing(Some(signal)));
        assert_eq!(
            facts().session,
            Session::Playing(Some("bf6.exe".to_owned()))
        );

        // A game Windows tracks but does not name is still a game.
        sink(&Engine::Playing(None));
        assert_eq!(facts().session, Session::Playing(None));
        assert_eq!(current_state(), State::Active);
        assert!(tooltip().contains("does not name"));

        sink(&Engine::Idle);
        assert_eq!(facts().session, Session::Idle);
        assert_eq!(current_state(), State::Idle);
        assert!(tooltip().contains("no game"));
    }

    /// The crash this module was rewritten for: a right-click re-enters the
    /// window procedure, so `dispatch` must never be inside a borrow when it
    /// happens. With no tray installed it should simply decline, twice over,
    /// rather than panic.
    #[test]
    fn dispatch_survives_being_re_entered() {
        let outer = dispatch(WM_TRAY, WPARAM(0), LPARAM(WM_CONTEXTMENU as isize));
        let inner = dispatch(WM_TRAY, WPARAM(0), LPARAM(WM_CONTEXTMENU as isize));
        assert!(outer.is_none() && inner.is_none());
    }
}
