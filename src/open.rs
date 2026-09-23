//! Opening a file, a folder or an address the way the user's settings say,
//! for the entries of the menu -- in a helper process that ends.
//!
//! `ShellExecute` loads the shell's machinery into the process that calls it
//! and leaves it there: 141 handles and 1.2 MB for the rest of the process's
//! life, measured on 2026-09-23, and a first open of another kind of file
//! adds more (`docs/design/16-footprint.md`). The watcher lives for days, so
//! it never calls the shell itself: it starts its own executable with the
//! hidden `open` command, which makes that one call and exits, and the cost
//! goes with it. A plain `CreateProcess` leaves two handles.
//!
//! The helper does what Microsoft documents for a caller that exits right
//! after: COM initialised as a single-threaded apartment before the shell is
//! called, and `ShellExecuteExW` with `SEE_MASK_NOASYNC`, so that the launch
//! has finished when the process ends.
//! <https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-shellexecuteinfow>

use std::path::Path;

use anyhow::{Context, Result};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_LOG_USAGE, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, SW_SHOWNORMAL};
use windows::core::PCWSTR;

use crate::logging::target;

/// From the watcher: have the helper open `what`, and say in the log how
/// it went once it has. Returns at once; the menu is not held up.
pub fn open(what: &str) {
    use std::os::windows::process::CommandExt;
    /// CREATE_NO_WINDOW: no console, should the watcher be the console twin.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let spawned = std::env::current_exe()
        .context("cannot find this executable")
        .and_then(|exe| {
            std::process::Command::new(exe)
                .arg("open")
                .arg(what)
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .context("cannot start the helper")
        });
    let mut helper = match spawned {
        Ok(helper) => helper,
        Err(error) => {
            tracing::warn!(
                target: target::WATCHER,
                what,
                error = %format!("{error:#}"),
                "Could not open it: the helper that opens files did not start"
            );
            return;
        }
    };
    // The window the helper opens is the user's answer to a click, and should
    // come to the front. The watcher may pass that right on: the menu made it
    // the foreground process.
    // SAFETY: a process id, no pointers.
    let _ = unsafe { AllowSetForegroundWindow(helper.id()) };
    let what = what.to_owned();
    std::thread::spawn(move || match helper.wait() {
        Ok(status) if status.success() => {
            tracing::debug!(target: target::WATCHER, what, "Opened");
        }
        Ok(status) => tracing::warn!(
            target: target::WATCHER,
            what,
            code = status.code(),
            "Nothing opened: the shell would not take it, nor Notepad for a file"
        ),
        Err(error) => tracing::debug!(
            target: target::WATCHER,
            what,
            error = %error,
            "The helper that opens files could not be waited for"
        ),
    });
}

/// In the helper: open `what` through the shell, or -- for a file the
/// shell has no program for, the likely miss being a `.toml` -- in Notepad,
/// since a menu entry that silently does nothing is worse than a plain
/// editor.
pub fn open_here(what: &str) -> Result<()> {
    // SAFETY: no pointers; paired with the uninitialise below, on this thread.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let opened = execute(what, None).or_else(|refused| {
        if Path::new(what).is_file() {
            execute("notepad.exe", Some(&format!("\"{what}\"")))
        } else {
            Err(refused)
        }
    });
    if com.is_ok() {
        // SAFETY: COM was initialised above on this thread, and nothing of
        // it is used after this point.
        unsafe { CoUninitialize() };
    }
    opened
}

fn execute(file: &str, parameters: Option<&str>) -> Result<()> {
    let verb = wide("open");
    let file_wide = wide(file);
    let parameters_wide = parameters.map(wide);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        // NOASYNC: this process ends right after. LOG_USAGE: a launch the
        // user asked for, as Microsoft asks such calls to say.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_LOG_USAGE,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file_wide.as_ptr()),
        lpParameters: parameters_wide
            .as_ref()
            .map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: the structure carries its own size, and every string it points
    // to is NUL-terminated and outlives the call.
    unsafe { ShellExecuteExW(&mut info) }.with_context(|| format!("the shell refused `{file}`"))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
