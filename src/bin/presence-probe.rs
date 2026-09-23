//! The measuring instrument behind the detection design.
//!
//! Windows exposes one documented extension point that says when *Windows
//! itself* considers a game present: the Game Bar Presence Writer.
//! <https://learn.microsoft.com/en-us/windows/win32/devnotes/gamebar-presencewriter>
//!
//! Registering a custom writer is impossible -- the registration key is owned
//! by TrustedInstaller -- so this tool only ever *observes* the one Windows
//! registered. Which executable that is comes from the registry, never from a
//! hard-coded name, so a machine where something else owns the registration
//! is probed correctly. Nothing here modifies the system or needs
//! administrator rights. `docs/design/00-detection.md` has what it measured.
//!
//! Usage:
//!
//! ```text
//! presence-probe status              the current registration and the log path
//! presence-probe watch [secs]        log when Windows' own presence writer runs
//! presence-probe watch-games [secs] [key]
//!                                    log what Windows writes to its game list, and when;
//!                                    `key`, under HKCU, checks the probe on one you can write
//! presence-probe activate            activate the class ourselves and time it
//! ```

#[cfg(not(windows))]
compile_error!("GameModeExecutor only targets Windows");

use std::io::Write;
use std::path::PathBuf;

use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::HSTRING;

use game_mode_executor::detect::presence_writer;

/// The runtime class Windows activates when game presence changes.
const CLASS_ID: &str = presence_writer::CLASS_ID;

// ---------------------------------------------------------------- logging --

/// Windows starts the server with no console, so everything goes to a file
/// next to the executable, falling back to the temp directory.
fn log_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        return dir.join("presence-probe.log");
    }
    std::env::temp_dir().join("presence-probe.log")
}

fn log(message: &str) {
    let line = format!("{} {message}\n", timestamp());
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = file.write_all(line.as_bytes());
    }
    // Harmless when there is no console.
    print!("{line}");
    let _ = std::io::stdout().flush();
}

fn timestamp() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: `GetLocalTime` takes no input and only returns a struct.
    let now = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond, now.wMilliseconds
    )
}

// ------------------------------------------------- observing the default --

/// Which executable to probe, and whether it is still the shipped one. Read
/// from the registry rather than hard-coded, so a machine where something else
/// has taken over the registration is probed correctly.
fn writer_exe() -> std::path::PathBuf {
    match presence_writer::registered_exe() {
        Ok(exe) => {
            if !presence_writer::is_microsoft_default(&exe) {
                log(&format!(
                    "note: the registration is NOT the Microsoft default; probing {} instead",
                    exe.display()
                ));
            }
            exe
        }
        Err(error) => {
            log(&format!(
                "warning: cannot read the registration ({error:#}); falling back to {}",
                presence_writer::MICROSOFT_DEFAULT
            ));
            std::path::PathBuf::from(presence_writer::MICROSOFT_DEFAULT)
        }
    }
}

/// The process owning the foreground window, to correlate a presence writer
/// launch with whatever the user was doing.
fn foreground_process_name() -> String {
    let Some(pid) = game_mode_executor::detect::process::foreground_pid() else {
        return "(none)".to_owned();
    };
    let name = game_mode_executor::detect::process::Snapshot::take()
        .ok()
        .and_then(|snapshot| snapshot.by_pid(pid).map(|process| process.name.clone()))
        .unwrap_or_else(|| "(unknown)".to_owned());
    format!("{name} (pid {pid})")
}

/// Purely passive: log when the registered presence writer comes and goes, and
/// alongside it the game process itself. Nothing is modified, nothing needs
/// admin. Run it, then play a game.
///
/// Tracking both is the point: it splits the delay a user feels when closing a
/// game into the part where the game is still shutting down and the part where
/// Windows is still holding its presence reference.
fn cmd_watch(seconds: u64) -> windows::core::Result<()> {
    use game_mode_executor::detect::known_games::KnownGames;
    use game_mode_executor::detect::process::Snapshot;

    let exe = writer_exe();
    let known = match KnownGames::load() {
        Ok(known) => Some(known),
        Err(error) => {
            log(&format!(
                "watch: known game list unavailable ({error:#}); the game process will not be tracked"
            ));
            None
        }
    };

    let mut writer: Option<u32> = None;
    let mut game: Option<(u32, String)> = None;
    let mut game_left_at: Option<std::time::Instant> = None;

    log(&format!("watch: watching {} for {seconds}s", exe.display()));

    // 200ms is a compromise: fast enough not to miss a brief launch, slow
    // enough that one process snapshot per tick stays cheap while a game runs.
    let started = std::time::Instant::now();
    let mut first = true;
    while started.elapsed().as_secs() < seconds {
        if !first {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        first = false;
        let at = started.elapsed().as_secs_f32();

        let Ok(snapshot) = Snapshot::take() else {
            continue;
        };
        let current_writer = presence_writer::find_in(&snapshot, &exe);

        // The game is identified once, when the writer appears; after that it
        // is only checked for still being in the snapshot, which is cheap.
        if game.is_none()
            && current_writer.is_some()
            && let Some(known) = &known
            && let Some(signal) = known.identify(&snapshot)
            && let (Some(pid), Some(name)) = (signal.process_id, signal.process_name.clone())
        {
            log(&format!(
                "watch: GAME {name} (pid {pid}) identified at +{at:.1}s"
            ));
            game = Some((pid, name));
        }
        if let Some((pid, name)) = &game
            && snapshot.by_pid(*pid).is_none()
        {
            log(&format!(
                "watch: GAME {name} (pid {pid}) EXITED at +{at:.1}s"
            ));
            game_left_at = Some(std::time::Instant::now());
            game = None;
        }

        match (writer, current_writer) {
            (None, Some(pid)) => log(&format!(
                "watch: WRITER STARTED pid {pid} at +{at:.1}s, foreground = {}",
                foreground_process_name()
            )),
            (Some(old), None) => {
                let gap = match game_left_at {
                    Some(when) => format!(
                        ", {:.1}s after the game process itself exited",
                        when.elapsed().as_secs_f32()
                    ),
                    None => ", the game process was never identified".to_owned(),
                };
                log(&format!(
                    "watch: WRITER EXITED pid {old} at +{at:.1}s{gap}, foreground = {}",
                    foreground_process_name()
                ));
                game_left_at = None;
            }
            (Some(old), Some(new)) if old != new => log(&format!(
                "watch: WRITER RESTARTED pid {old} -> {new} at +{at:.1}s"
            )),
            _ => {}
        }
        writer = current_writer;
    }
    log("watch: done");
    Ok(())
}

/// Activate the runtime class ourselves, to find out whether Windows starts
/// the writer on demand and how long it lingers once nobody holds it.
fn cmd_activate(hold: u64, linger: u64) -> windows::core::Result<()> {
    use windows::Win32::System::WinRT::RoActivateInstance;

    // SAFETY: initialises the Windows Runtime for this thread; no pointers.
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };

    let exe = writer_exe();
    let before = presence_writer::running_pid(&exe);
    log(&format!(
        "activate: {} before = {}",
        exe.display(),
        match before {
            Some(pid) => format!("running (pid {pid})"),
            None => "not running".to_owned(),
        }
    ));

    let started = std::time::Instant::now();
    // SAFETY: the class id is a valid HSTRING that outlives the call.
    let instance = unsafe { RoActivateInstance(&HSTRING::from(CLASS_ID)) };
    match &instance {
        Ok(object) => {
            log(&format!(
                "activate: RoActivateInstance succeeded in {:.0}ms",
                started.elapsed().as_secs_f32() * 1000.0
            ));
            match object.GetRuntimeClassName() {
                Ok(name) => log(&format!("activate: runtime class name = {name}")),
                Err(error) => log(&format!("activate: GetRuntimeClassName failed: {error}")),
            }
        }
        Err(error) => log(&format!("activate: RoActivateInstance failed: {error}")),
    }

    // Did a process appear, and how quickly?
    let appeared = std::time::Instant::now();
    let mut spawned = None;
    while appeared.elapsed().as_secs() < 5 {
        if let Some(pid) = presence_writer::running_pid(&exe)
            && before != Some(pid)
        {
            spawned = Some(pid);
            log(&format!(
                "activate: {} appeared as pid {pid} after {:.0}ms",
                exe.display(),
                appeared.elapsed().as_secs_f32() * 1000.0
            ));
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if spawned.is_none() {
        log("activate: no new presence writer process appeared within 5s");
    }

    log(&format!("activate: holding the object for {hold}s"));
    std::thread::sleep(std::time::Duration::from_secs(hold));
    drop(instance);
    log("activate: released; measuring how long the process lingers");

    let released = std::time::Instant::now();
    while released.elapsed().as_secs() < linger {
        if presence_writer::running_pid(&exe).is_none() {
            log(&format!(
                "activate: process exited {:.1}s after release",
                released.elapsed().as_secs_f32()
            ));
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    log(&format!("activate: still running {linger}s after release"));
    Ok(())
}

// --------------------------------------------------------------- commands --

fn cmd_status() -> windows::core::Result<()> {
    match presence_writer::registered_exe() {
        Ok(exe) => {
            println!("Registration      : {}", exe.display());
            println!(
                "  Microsoft default : {}",
                if presence_writer::is_microsoft_default(&exe) {
                    "yes"
                } else {
                    "NO - something else owns the registration"
                }
            );
            match presence_writer::running_pid(&exe) {
                Some(pid) => println!("Writer running    : yes (pid {pid})"),
                None => println!("Writer running    : no"),
            }
        }
        Err(error) => println!("Registration      : unreadable ({error:#})"),
    }
    println!("Log file          : {}", log_path().display());
    Ok(())
}

// ------------------------------------------------ watching the game list --

/// One entry of Windows' game list, as much of it as the probe compares.
#[derive(Clone, PartialEq, Eq)]
struct Entry {
    name: String,
    exe: Option<String>,
    package: Option<String>,
    revision: Option<u32>,
    title_id: Option<String>,
    last_accessed: Option<u64>,
}

impl Entry {
    fn label(&self) -> String {
        let what = self
            .exe
            .as_deref()
            .map(|exe| exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_owned())
            .or_else(|| self.package.clone())
            .unwrap_or_else(|| "(no exe, no package)".to_owned());
        let kind = match (self.revision, &self.title_id) {
            (Some(1), None) => "hand-made",
            (_, Some(_)) => "listed, with a title id",
            _ => "listed, no title id",
        };
        format!("{what} [{kind}, {}]", self.name)
    }
}

/// Windows' game list, per user.
const GAME_LIST: &str = r"System\GameConfigStore\Children";

fn read_entries(key: &str) -> Vec<Entry> {
    use game_mode_executor::registry::Key;
    let Ok(root) = Key::open_current_user(key) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = root
        .subkey_names()
        .into_iter()
        .filter_map(|name| {
            let child = root.open_subkey(&name).ok()?;
            Some(Entry {
                exe: child.string_value("MatchedExeFullPath"),
                package: child.string_value("UtmItemId"),
                revision: child.dword_value("Revision"),
                title_id: child
                    .string_value("TitleId")
                    .or_else(|| child.dword_value("TitleId").map(|id| id.to_string())),
                last_accessed: child.qword_value("LastAccessed"),
                name,
            })
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// How long ago a `FILETIME` was, against the clock now.
fn ago(filetime: u64) -> String {
    use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
    // SAFETY: takes no input and only returns a struct.
    let now = unsafe { GetSystemTimeAsFileTime() };
    let now = (u64::from(now.dwHighDateTime) << 32) | u64::from(now.dwLowDateTime);
    let delta = now.abs_diff(filetime) as f64 / 10_000_000.0;
    if now >= filetime {
        format!("{delta:.1}s ago")
    } else {
        format!("{delta:.1}s ahead")
    }
}

/// Say what moved between two reads of the list, each line tagged; returns
/// how many lines it said.
fn diff(before: &[Entry], after: &[Entry], tag: &str) -> usize {
    let mut said = 0;
    for entry in after {
        match before.iter().find(|old| old.name == entry.name) {
            None => {
                said += 1;
                log(&format!("watch-games: {tag} ADDED {}", entry.label()));
            }
            Some(old) if old.last_accessed != entry.last_accessed => {
                said += 1;
                log(&format!(
                    "watch-games: {tag} LastAccessed moved for {} -> {}",
                    entry.label(),
                    entry.last_accessed.map_or("(none)".to_owned(), ago)
                ));
            }
            Some(old) if old != entry => {
                said += 1;
                log(&format!(
                    "watch-games: {tag} changed (not LastAccessed) {}",
                    entry.label()
                ));
            }
            Some(_) => {}
        }
    }
    for old in before {
        if !after.iter().any(|entry| entry.name == old.name) {
            said += 1;
            log(&format!("watch-games: {tag} REMOVED {}", old.label()));
        }
    }
    said
}

/// Park on a change notification for Windows' game list and say what moved:
/// entries added, removed, or touched -- and which `LastAccessed` moved,
/// which is the question Lot 15 asks: does Windows touch the entry of every
/// game it detects at launch, listed or hand-made? The presence writer is
/// logged beside it, so the two signals can be read against each other.
/// Nothing is modified, nothing needs administrator rights. `key` is the
/// game list unless the probe itself is being checked against a key one
/// can write to.
fn cmd_watch_games(seconds: u64, key: &str) -> windows::core::Result<()> {
    use game_mode_executor::registry::Key;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Registry::{
        REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME, REG_NOTIFY_THREAD_AGNOSTIC,
        RegNotifyChangeKeyValue,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    let root = match Key::open_current_user(key) {
        Ok(root) => root,
        Err(error) => {
            log(&format!("watch-games: cannot open HKCU\\{key} ({error:#})"));
            return Ok(());
        }
    };
    let exe = writer_exe();
    // SAFETY: no security attributes, no name; closed at the end.
    let event = unsafe { CreateEventW(None, false, false, None) }?;

    let mut before = read_entries(key);
    let mut writer = presence_writer::running_pid(&exe);
    log(&format!(
        "watch-games: {} entries, {} hand-made (Revision 1, no TitleId); writer {}; watching for {seconds}s",
        before.len(),
        before
            .iter()
            .filter(|entry| entry.revision == Some(1) && entry.title_id.is_none())
            .count(),
        match writer {
            Some(pid) => format!("running (pid {pid})"),
            None => "not running".to_owned(),
        }
    ));

    // One-shot, and armed once per wake: each call while one is pending adds
    // another wait on the key, which Microsoft documents as a leak -- the
    // first version of this probe re-armed every 250 ms. THREAD_AGNOSTIC so
    // the event could be waited on from another thread, which the watcher
    // will.
    let arm = || {
        // SAFETY: `root` outlives every call, `event` is a live event handle.
        let armed = unsafe {
            RegNotifyChangeKeyValue(
                root.raw(),
                true,
                REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_THREAD_AGNOSTIC,
                Some(event),
                true,
            )
        };
        if armed.is_err() {
            log(&format!(
                "watch-games: RegNotifyChangeKeyValue failed ({armed:?})"
            ));
        }
        armed.is_ok()
    };
    if !arm() {
        return Ok(());
    }

    let started = std::time::Instant::now();
    let mut wakeups = 0u32;
    let mut missed = 0u32;
    while started.elapsed().as_secs() < seconds {
        // Wake every 250 ms anyway, to log the writer coming and going and
        // to read the list without being told: a change found that way is
        // one the notification did not deliver, which is the other question.
        // SAFETY: `event` is live for the whole loop.
        let woke = unsafe { WaitForSingleObject(event, 250) } == WAIT_OBJECT_0;
        let at = started.elapsed().as_secs_f32();

        let current_writer = presence_writer::running_pid(&exe);
        match (writer, current_writer) {
            (None, Some(pid)) => log(&format!(
                "watch-games: WRITER STARTED pid {pid} at +{at:.1}s"
            )),
            (Some(pid), None) => log(&format!(
                "watch-games: WRITER EXITED pid {pid} at +{at:.1}s"
            )),
            _ => {}
        }
        writer = current_writer;

        let after = read_entries(key);
        let tag = if woke {
            wakeups += 1;
            format!("#{wakeups} +{at:.1}s")
        } else {
            format!("MISSED +{at:.1}s, no notification:")
        };
        let said = diff(&before, &after, &tag);
        if !woke && said > 0 {
            missed += 1;
        }
        if woke && said == 0 {
            log(&format!(
                "watch-games: {tag} notification, nothing the probe compares moved"
            ));
        }
        before = after;
        if woke && !arm() {
            break;
        }
    }
    log(&format!(
        "watch-games: done, {wakeups} wake-ups and {missed} changes found without one in {seconds}s"
    ));
    // SAFETY: the event created above, closed once.
    unsafe { _ = CloseHandle(event) };
    Ok(())
}

fn main() -> windows::core::Result<()> {
    let seconds = || {
        std::env::args()
            .nth(2)
            .and_then(|value| value.parse().ok())
            .unwrap_or(600)
    };
    match std::env::args().nth(1).as_deref() {
        None | Some("status") => cmd_status(),
        Some("watch") => cmd_watch(seconds()),
        Some("watch-games") => {
            let key = std::env::args().nth(3);
            cmd_watch_games(seconds(), key.as_deref().unwrap_or(GAME_LIST))
        }
        Some("activate") => cmd_activate(5, 60),
        Some(other) => {
            eprintln!("unknown command `{other}`");
            eprintln!(
                "usage: presence-probe [status|watch [seconds]|watch-games [seconds]|activate]"
            );
            std::process::exit(2);
        }
    }
}
