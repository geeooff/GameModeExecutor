//! The notification area icon and its menu.
//!
//! Hangs off the window Lot 5 created, which already exists for
//! `WM_QUERYENDSESSION` and already pumps messages on the main thread. The
//! window procedure in `win` hands anything it does not handle itself to
//! [`dispatch`].
//!
//! State lives in a thread local rather than a mutex: everything here runs on
//! the one thread that owns the window, and saying so in the type is better
//! than locking against contention that cannot happen.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
    NOTIFY_ICON_DATA_FLAGS, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
    ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, DestroyIcon, DestroyMenu,
    GetSystemMetrics, HICON, IMAGE_FLAGS, LR_DEFAULTCOLOR, MF_SEPARATOR, MF_STRING, PostMessageW,
    RegisterWindowMessageW, SM_CXSMICON, SM_CYSMICON, SW_SHOWNORMAL, SetForegroundWindow,
    TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_APP, WM_COMMAND, WM_CONTEXTMENU, WM_DPICHANGED, WM_NULL,
    WM_SETTINGCHANGE,
};
use windows::core::PCWSTR;

use crate::win::StopSignal;

/// Our callback message. `WM_APP + 1` is the watcher-finished message in `win`.
const WM_TRAY: u32 = WM_APP + 2;

const ID_CONFIG: usize = 1;
const ID_LOG: usize = 2;
const ID_DOCS: usize = 3;
const ID_QUIT: usize = 4;

/// One icon per state and taskbar theme, compiled in.
///
/// Embedded rather than loaded from disk so a portable copy is one folder with
/// nothing to lose. It costs about 190 KB across the six, measured, which is
/// the price of never having to find a file at runtime.
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
    /// Reserved: nothing sets this yet, and the engine has no notion of a
    /// standing error. The artwork exists so the meaning is already spoken for
    /// and nobody reaches for the slash to mean something else.
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
    state: State,
    theme: Theme,
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

/// Add the icon. Call once, from the thread owning `window`.
pub fn install(window: isize, targets: Targets, stop: Arc<StopSignal>) -> Result<()> {
    let window = HWND(window as *mut std::ffi::c_void);
    let theme = Theme::current();
    let icon =
        load_icon(State::Idle, theme, window).context("cannot build the notification icon")?;
    let taskbar_created = unsafe { RegisterWindowMessageW(w("TaskbarCreated")) };

    let tray = Tray {
        window,
        icon,
        state: State::Idle,
        theme,
        targets,
        stop,
        taskbar_created,
    };
    tray.add()?;
    TRAY.with(|cell| *cell.borrow_mut() = Some(tray));
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
    TRAY.with(|cell| {
        if let Some(tray) = cell.borrow_mut().take() {
            tray.remove();
        }
    });
}

/// Messages `win`'s window procedure did not handle. Returns `Some` when this
/// module dealt with one.
pub fn dispatch(message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    TRAY.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let tray = borrowed.as_mut()?;
        tray.handle(message, wparam, lparam)
    })
}

impl Tray {
    fn handle(&mut self, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        if message == self.taskbar_created {
            // Explorer came back and took every icon with it when it went.
            let _ = self.add();
            tracing::debug!(
                target: crate::logging::target::WATCHER,
                "Explorer restarted, notification icon added again"
            );
            return Some(LRESULT(0));
        }

        match message {
            WM_TRAY => {
                // With NOTIFYICON_VERSION_4 the event is in the low word of
                // lParam and the cursor position is in wParam, which is why the
                // version is set at all: the old packing had no room for both.
                if (lparam.0 as u32) & 0xFFFF == WM_CONTEXTMENU {
                    let x = (wparam.0 & 0xFFFF) as i16 as i32;
                    let y = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32;
                    self.show_menu(POINT { x, y });
                }
                Some(LRESULT(0))
            }
            WM_COMMAND => {
                self.command(wparam.0 & 0xFFFF);
                Some(LRESULT(0))
            }
            // The taskbar theme changed under us.
            WM_SETTINGCHANGE => {
                if setting_is(lparam, "ImmersiveColorSet") {
                    self.refresh();
                }
                None
            }
            // A different monitor, or a scaling change: the icon Windows wants
            // is a different size now, and the old one would be resampled.
            WM_DPICHANGED => {
                self.refresh();
                None
            }
            _ => None,
        }
    }

    fn command(&mut self, id: usize) {
        match id {
            ID_CONFIG => self.open(&self.targets.config.clone()),
            ID_LOG => self.open(&self.targets.log.clone()),
            ID_DOCS => open_url(crate::build_info::DOCS_URL),
            ID_QUIT => {
                tracing::info!(
                    target: crate::logging::target::WATCHER,
                    "Quit chosen from the notification icon"
                );
                // Same path as Ctrl-C and as logging off: the engine unwinds,
                // the stop commands run, the message loop ends on its own.
                self.stop.signal();
            }
            _ => {}
        }
    }

    /// Open a file the way the user's own settings say to.
    fn open(&self, path: &std::path::Path) {
        let wide = w_string(&path.to_string_lossy());
        let result = unsafe {
            ShellExecuteW(
                Some(self.window),
                w("open"),
                PCWSTR(wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // ShellExecuteW returns a fake HINSTANCE; anything at or below 32 is an
        // error code. A `.toml` with no association is the likely one, so fall
        // back rather than leaving the menu entry silently doing nothing.
        if result.0 as usize <= 32 {
            tracing::debug!(
                target: crate::logging::target::WATCHER,
                path = %path.display(),
                "No association for this file, opening it in Notepad"
            );
            let arg = w_string(&format!("\"{}\"", path.display()));
            unsafe {
                let _ = ShellExecuteW(
                    Some(self.window),
                    w("open"),
                    w("notepad.exe"),
                    PCWSTR(arg.as_ptr()),
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                );
            }
        }
    }

    fn show_menu(&self, at: POINT) {
        let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
            return;
        };
        unsafe {
            let _ = AppendMenuW(menu, MF_STRING, ID_CONFIG, w("Edit configuration"));
            let _ = AppendMenuW(menu, MF_STRING, ID_LOG, w("Open log"));
            let _ = AppendMenuW(menu, MF_STRING, ID_DOCS, w("Documentation"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w("Quit"));

            // Documented requirement: without it the menu stays on screen when
            // the user clicks elsewhere, because the owner window is not
            // foreground and never learns it lost the click.
            let _ = SetForegroundWindow(self.window);
            let _ = TrackPopupMenuEx(menu, TPM_RIGHTBUTTON.0, at.x, at.y, self.window, None);
            // The other half of the same workaround.
            let _ = PostMessageW(Some(self.window), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
        }
    }

    /// Re-read the theme and rebuild the icon at the size Windows wants now.
    fn refresh(&mut self) {
        let theme = Theme::current();
        let Ok(icon) = load_icon(self.state, theme, self.window) else {
            return;
        };
        let previous = std::mem::replace(&mut self.icon, icon);
        self.theme = theme;
        let _ = self.modify();
        unsafe { _ = DestroyIcon(previous) };
        tracing::debug!(
            target: crate::logging::target::WATCHER,
            theme = ?theme,
            state = ?self.state,
            "Notification icon reloaded"
        );
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
        let tip = w_string(self.tooltip());
        let len = tip.len().min(data.szTip.len());
        data.szTip[..len].copy_from_slice(&tip[..len]);
        data
    }

    fn tooltip(&self) -> &'static str {
        match self.state {
            State::Idle => "GameModeExecutor - watching, no game detected",
            State::Active => "GameModeExecutor - a game is running",
            State::Error => "GameModeExecutor - something needs attention",
        }
    }

    fn add(&self) -> Result<()> {
        let data = self.data();
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &data)
                .ok()
                .context("Shell_NotifyIcon could not add the icon")?;
            // Opt into the version 4 behaviour. Without this the callback
            // arrives in the old packing and the coordinates are wrong.
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
        Ok(())
    }

    fn modify(&self) -> Result<()> {
        let data = self.data();
        unsafe {
            Shell_NotifyIconW(windows::Win32::UI::Shell::NIM_MODIFY, &data)
                .ok()
                .context("Shell_NotifyIcon could not update the icon")?;
        }
        Ok(())
    }

    fn remove(&self) {
        let data = self.data();
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            let _ = DestroyIcon(self.icon);
        }
    }
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
    let dpi = unsafe { GetDpiForWindow(window) };
    if dpi == 0 {
        // No window yet, or an older Windows. The unscaled values are still
        // better than nothing.
        return unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };
    }
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
        let unit = unsafe { *pointer.add(offset) };
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    String::from_utf16_lossy(&units) == name
}

fn open_url(url: &str) {
    let wide = w_string(url);
    unsafe {
        let _ = ShellExecuteW(
            None,
            w("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// A null-terminated UTF-16 buffer, kept alive by the caller.
fn w_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A literal for an API call, valid for the duration of that call.
fn w(value: &str) -> PCWSTR {
    // Leaked on purpose: these are a handful of fixed strings created once,
    // and the alternative is a lifetime dance around every Win32 call.
    let buffer: &'static [u16] = Box::leak(w_string(value).into_boxed_slice());
    PCWSTR(buffer.as_ptr())
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
}
