# Lot 6 — Notification area icon

**Goal.** A notification area icon with a small context menu — edit the
configuration, open the log, open the documentation, quit — hung off the
window Lot 5 created.

**Done when:** the icon appears at logon in the right variant for the taskbar
theme, the menu entries open the right files in the user's own programs, Quit
exits cleanly, and the icon survives killing and restarting Explorer. Closed
2026-09-15.

- [x] `Shell_NotifyIcon` with `NOTIFYICON_VERSION_4`, on the Lot 5 window
- [x] Context menu: edit configuration · open log · documentation · quit
- [x] All three opened through `ShellExecuteW`, with a Notepad fallback when nothing claims `.toml`
- [x] The documentation entry opens the build's own commit on GitHub
- [x] Re-added on `TaskbarCreated`, so an Explorer restart does not lose it
- [x] Follows the taskbar theme, and re-reads it when the menu is about to open
- [x] Reloaded on `WM_DPICHANGED`
- [x] Per-monitor DPI awareness declared — a real defect, below
- [x] Quit shuts the watcher down cleanly, stop actions included
- [x] Failing to add the icon is a warning; the watcher carries on
- [x] Icons in the repository as `.ico` and `.svg`, eight frames each, C2PA-free — see `assets/icons/README.md`
- [x] The active icon compiled into every executable through the Windows SDK's `rc.exe` from `build.rs`; a missing SDK is a warning, not a failed build

## On "modern APIs, no exotic libraries"

For a notification area icon that means `Shell_NotifyIcon` with
`NOTIFYICON_VERSION_4`. There is no newer replacement — WinUI and WinRT do not
offer tray icons at all — so this is the current supported API rather than a
legacy one. Modern here means the official Microsoft `windows` crate bindings,
already a dependency, and no third-party tray wrapper.

Impact was measured before committing: `user32`, `gdi32`, `shell32` and
`combase` were already loaded, so no new DLL; well under 1 MB of extra private
bytes, no measurable CPU, +30–60 KB of binary. `LoadIconMetric` was avoided
because it would pull in `comctl32.dll`.

An icon must be a PE resource, and `rc.exe` is the Microsoft tool that makes
one; no crate is involved. `LookupIconIdFromDirectoryEx` cannot read an `.ico`
*file*: it expects `RT_GROUP_ICON` resource data, whose entries hold resource
ids where a file's hold byte offsets — one header, two layouts — so the frame
picker for the tray is written out by hand.

## The one undocumented call in the program

A menu built with `TrackPopupMenuEx` renders light whatever the taskbar is set
to, and **there is no documented way to change that**. The Bluetooth icon's
menu, two slots along in the same tray, is dark — and it is a plain Win32 menu,
not a XAML surface. What such applications do, Explorer included, is call
`SetPreferredAppMode` in `uxtheme.dll`: undocumented, exported by ordinal only,
not exported by name at all on Windows 11. Microsoft has an open request for a
supported replacement; it has not landed.

This project has turned workarounds down before — MSIX, `AttachConsole`, WiX.
The distinction that lets this one in is the failure mode. Those failed
*silently and wrongly*: exit codes that stop reaching scripts, a log written
where nobody can read it. This one fails visibly and harmlessly: the ordinal
moves, the menu is light again, nothing else changes.

Guarded at every step: the library may not load, the build may predate 1903
(where the same ordinal is a different function taking a different argument),
the export may be gone. Each means no call and a light menu. The build number
comes from the registry rather than `GetVersionEx`, which lies about anything
past Windows 8 without a compatibility manifest.

### Themes that move after startup

Setting the mode once is not enough, and the hole is not where it looks. The
theme is re-read and the menu theme cache flushed **when the menu is about to
be built**, not only when `WM_SETTINGCHANGE` arrives — because that message is
not guaranteed. A tool that switches light and dark on a schedule may write the
registry and broadcast nothing. Demonstrated on 2026-09-15:

| Case | What the log showed |
| --- | --- |
| registry written directly, no broadcast | **0 log lines** — the watcher hears nothing |
| menu then opened | `icon refreshed theme=Light` — the drift is caught |
| menu opened again, nothing changed | **0 log lines** — no redraw, no noise |

The cost is one registry read per right-click. What it removes is a whole
class of problem: correctness no longer depends on having been told. The third
row is the other half: the tray remembers exactly what the shell is showing —
state, theme and tooltip — so a refresh that would change nothing does nothing.

Windows also broadcasts `ImmersiveColorSet` **twice** for one switch, about
150 ms apart, and whether the first one already carries the new value is not
reliable. The count can be relied on; the timing cannot. Nothing breaks
because `reload` compares wanted against shown.

## The crash a right-click caused

The first build put the icon up correctly and died the moment anyone
right-clicked it. `TrackPopupMenuEx` is modal: it runs its own message loop
while the menu is open, so the window procedure is re-entered and `dispatch`
is called again — inside a `RefCell` borrow that was still held. A second
`borrow_mut` panics, and `panic = "abort"` turns that into `0xC0000409` with
no log line at all.

**The module is now shaped by the rule.** Every message becomes a `Plan` under
a short borrow, and the plan is carried out with nothing borrowed. The same
applies to `ShellExecuteW`, which can show UI of its own. `TPM_RETURNCMD` with
`TPM_NONOTIFY` removes the second re-entrant path, where the menu posts
`WM_COMMAND` to the window while its loop is still running.

**A crash now says so.** A panic hook logs `FATAL:` with the build and the
location — see [Lot 4](04-logging.md) for why the word is in the message and
the log is written synchronously.

Verified by posting the exact message the shell sends on a right-click to the
running watcher, and watching it stay up.

## The bug only a measurement would have caught

The first run logged `Notification icon added theme=Dark size=16` on a display
set to 150 %, where the shell wants 24. The process was DPI-unaware, so
`GetSystemMetrics` answered with the 96 dpi value whatever the display said,
and Windows then stretched a 16 pixel icon to 24 — precisely the soft result
the design notes said to avoid by shipping eight hand-tuned frames.

Two halves to the fix: `SetProcessDpiAwarenessContext` with
`PER_MONITOR_AWARE_V2` before any window exists, and `GetSystemMetricsForDpi`
with the window's own dpi. It now logs `size=24` and Windows gets the frame
drawn for it. A stretched icon is not an error, just worse; it was found
because the size is logged at all.

## The artwork

The idle icon first carried a diagonal slash, which in Windows iconography
reads as *disabled* — and idle is the state the program spends nearly all its
time in. The slash moved to a distinct **error** state, reserved and unused
until [Lot 9](09-robustness.md) gives it a meaning, so nobody borrows it for
anything else. The frames were checked rather than trusted: eight PNG frames
per `.ico` at 32-bit alpha, no C2PA payload, and the four luminance figures
from the design notes reproduce exactly.
