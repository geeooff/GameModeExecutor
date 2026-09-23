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
//! presence-probe watch-methods <secs> <sid>
//!                                    several ways of being told the game list changed, at once
//! presence-probe cost [rounds]       time what an idle poll costs, today and with Lot 15
//! presence-probe footprint           what each step of the watcher leaves in memory and handles
//! presence-probe menu-cost <open|check|spawn|menu|menu-dark>
//!                                    what one click of the menu leaves, over five minutes,
//!                                    or the menu itself, light or dark, shown three times
//! presence-probe gpu-load [ms] [rounds]
//!                                    the rendering load the refinement reads, busiest first
//! presence-probe microsoft-list <exe path>...
//!                                    whether Microsoft's own game list covers each executable
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

// ------------------------------------------------------ what a poll costs --

/// Median, 95th percentile and maximum of `samples`, in microseconds.
fn spread(samples: &mut [std::time::Duration]) -> String {
    samples.sort();
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    format!(
        "median {:>6.0} us, p95 {:>6.0} us, max {:>6.0} us",
        at(0.5).as_secs_f64() * 1e6,
        at(0.95).as_secs_f64() * 1e6,
        at(1.0).as_secs_f64() * 1e6
    )
}

/// Time, over `rounds` rounds, what the watcher's idle poll does today and
/// what Lot 15's second signal would add to it: the process snapshot the
/// poll already takes, the writer's lookup in it, the hand-made entries'
/// file names compared against the same snapshot, one full-path query --
/// what a name that matches would cost -- and the last-write time of the
/// list's key, which says whether the list must be read again.
fn cmd_cost(rounds: usize) -> windows::core::Result<()> {
    use game_mode_executor::detect::process::{Snapshot, full_path};
    use game_mode_executor::registry::Key;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Registry::RegQueryInfoKeyW;

    let exe = writer_exe();
    let hand_made: Vec<String> = read_entries(GAME_LIST)
        .into_iter()
        .filter(|entry| entry.revision == Some(1) && entry.title_id.is_none())
        .filter_map(|entry| entry.exe)
        .filter_map(|exe| exe.rsplit(['\\', '/']).next().map(str::to_ascii_lowercase))
        .collect();
    let Ok(root) = Key::open_current_user(GAME_LIST) else {
        log("cost: cannot open the game list");
        return Ok(());
    };

    let mut snapshot = Vec::with_capacity(rounds);
    let mut writer = Vec::with_capacity(rounds);
    let mut names = Vec::with_capacity(rounds);
    let mut path = Vec::with_capacity(rounds);
    let mut stamp = Vec::with_capacity(rounds);
    let mut ids = Vec::with_capacity(rounds);
    let mut processes = 0;
    let mut matched = 0;
    for _ in 0..rounds {
        let started = Instant::now();
        let Ok(taken) = Snapshot::take() else {
            continue;
        };
        snapshot.push(started.elapsed());
        processes = taken.processes.len();

        let started = Instant::now();
        let _ = presence_writer::find_in(&taken, &exe);
        writer.push(started.elapsed());

        let started = Instant::now();
        matched = taken
            .processes
            .iter()
            .filter(|process| hand_made.contains(&process.name.to_ascii_lowercase()))
            .count();
        names.push(started.elapsed());

        let started = Instant::now();
        let _ = full_path(std::process::id());
        path.push(started.elapsed());

        let started = Instant::now();
        let mut written = FILETIME::default();
        // SAFETY: `root` is open for the call; only the last-write time is
        // asked for, into a local that outlives it.
        let _ = unsafe {
            RegQueryInfoKeyW(
                root.raw(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(&mut written),
            )
        };
        stamp.push(started.elapsed());

        // The process ids alone, as the watcher's idle look takes them.
        let started = Instant::now();
        let _ = game_mode_executor::detect::process::ids();
        ids.push(started.elapsed());

        std::thread::sleep(Duration::from_millis(10));
    }
    log(&format!(
        "cost: {} rounds, {processes} processes, {} hand-made entries, {matched} running",
        snapshot.len(),
        hand_made.len()
    ));
    log(&format!(
        "cost: process snapshot           {}",
        spread(&mut snapshot)
    ));
    log(&format!(
        "cost: writer found in it (today) {}",
        spread(&mut writer)
    ));
    log(&format!(
        "cost: hand-made names compared   {}",
        spread(&mut names)
    ));
    log(&format!(
        "cost: one full-path query        {}",
        spread(&mut path)
    ));
    log(&format!(
        "cost: the list's last-write time {}",
        spread(&mut stamp)
    ));
    log(&format!(
        "cost: process ids alone          {}",
        spread(&mut ids)
    ));
    Ok(())
}

// ------------------------------------ other ways of being told of a change --

/// Arm several ways of asking Windows to say when the game list changes, at
/// once, each on its own event, and log which of them fire -- beside the
/// changes found by reading the list every 250 ms. The first two runs used
/// one way only, the predefined `HKCU` handle over the list's subtree, and
/// it never fired for the Game Bar's writes; this asks whether another way
/// does. `sid` is the account's, for the `HKEY_USERS` path.
fn cmd_watch_methods(seconds: u64, sid: &str) -> windows::core::Result<()> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_USERS, KEY_READ, REG_NOTIFY_CHANGE_ATTRIBUTES,
        REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME, REG_NOTIFY_CHANGE_SECURITY,
        REG_NOTIFY_FILTER, REG_NOTIFY_THREAD_AGNOSTIC, RegNotifyChangeKeyValue, RegOpenCurrentUser,
        RegOpenKeyExW,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, WaitForMultipleObjects, WaitForSingleObject,
    };
    use windows::core::HSTRING;

    fn open(root: HKEY, path: &str) -> Option<HKEY> {
        let mut key = HKEY::default();
        // SAFETY: the name outlives the call and `key` is a local out
        // pointer. The keys are never closed: the process ends with them.
        let rc = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ, &mut key) };
        if rc.is_err() {
            log(&format!("methods: cannot open `{path}` ({rc:?})"));
            return None;
        }
        Some(key)
    }

    /// One way of being told: the keys it watches, how, and its event.
    struct Method {
        name: String,
        keys: Vec<HKEY>,
        subtree: bool,
        filter: REG_NOTIFY_FILTER,
        event: HANDLE,
        fired: u32,
    }

    impl Method {
        fn arm(&self) {
            for key in &self.keys {
                // SAFETY: the key is open for the life of the process and
                // the event outlives the loop.
                let armed = unsafe {
                    RegNotifyChangeKeyValue(
                        *key,
                        self.subtree,
                        self.filter | REG_NOTIFY_THREAD_AGNOSTIC,
                        Some(self.event),
                        true,
                    )
                };
                if armed.is_err() {
                    log(&format!(
                        "methods: {} could not be armed ({armed:?})",
                        self.name
                    ));
                }
            }
        }
    }

    let usual = REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET;
    let every = usual | REG_NOTIFY_CHANGE_ATTRIBUTES | REG_NOTIFY_CHANGE_SECURITY;
    let mut current_user = HKEY::default();
    // SAFETY: a local out pointer.
    let _ = unsafe { RegOpenCurrentUser(KEY_READ.0, &mut current_user) };
    let entries: Vec<HKEY> = read_entries(GAME_LIST)
        .iter()
        .filter_map(|entry| open(HKEY_CURRENT_USER, &format!(r"{GAME_LIST}\{}", entry.name)))
        .collect();

    let specs: Vec<(String, Vec<HKEY>, bool, REG_NOTIFY_FILTER)> = vec![
        (
            "1 HKCU, the list, subtree (the first two runs)".into(),
            open(HKEY_CURRENT_USER, GAME_LIST).into_iter().collect(),
            true,
            usual,
        ),
        (
            r"2 HKEY_USERS\<sid>, the list, subtree".into(),
            open(HKEY_USERS, &format!(r"{sid}\{GAME_LIST}"))
                .into_iter()
                .collect(),
            true,
            usual,
        ),
        (
            "3 RegOpenCurrentUser, the list, subtree".into(),
            open(current_user, GAME_LIST).into_iter().collect(),
            true,
            usual,
        ),
        (
            "4 HKCU, the list, its own subkeys only".into(),
            open(HKEY_CURRENT_USER, GAME_LIST).into_iter().collect(),
            false,
            usual,
        ),
        (
            "5 HKCU, GameConfigStore, subtree, every filter".into(),
            open(HKEY_CURRENT_USER, r"System\GameConfigStore")
                .into_iter()
                .collect(),
            true,
            every,
        ),
        (
            "6 HKCU, System, subtree, every filter".into(),
            open(HKEY_CURRENT_USER, "System").into_iter().collect(),
            true,
            every,
        ),
        (
            format!("7 each of the {} entries on its own", entries.len()),
            entries,
            false,
            every,
        ),
    ];
    let mut methods: Vec<Method> = specs
        .into_iter()
        .filter(|(_, keys, _, _)| !keys.is_empty())
        .map(|(name, keys, subtree, filter)| Method {
            name,
            keys,
            subtree,
            filter,
            // SAFETY: no security attributes, no name; auto-reset. Never
            // closed: the process ends with it.
            event: unsafe { CreateEventW(None, false, false, None) }.unwrap_or_default(),
            fired: 0,
        })
        .collect();
    for method in &methods {
        method.arm();
        log(&format!("methods: armed {}", method.name));
    }

    // 8: the same question asked synchronously, on a thread of its own that
    // blocks inside the call until it is answered.
    std::thread::spawn(|| {
        let Some(key) = open(HKEY_CURRENT_USER, GAME_LIST) else {
            return;
        };
        loop {
            // SAFETY: the key is open for the life of the process; no event,
            // the call blocks this thread until a change or an error.
            let answered = unsafe {
                RegNotifyChangeKeyValue(
                    key,
                    true,
                    REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET,
                    None,
                    false,
                )
            };
            log(&format!(
                "methods: FIRED 8 synchronous, HKCU, the list, subtree ({answered:?})"
            ));
            if answered.is_err() {
                return;
            }
        }
    });
    log("methods: armed 8 synchronous, HKCU, the list, subtree");

    let events: Vec<HANDLE> = methods.iter().map(|method| method.event).collect();
    let exe = writer_exe();
    let mut writer = presence_writer::running_pid(&exe);
    let mut before = read_entries(GAME_LIST);
    let started = std::time::Instant::now();
    while started.elapsed().as_secs() < seconds {
        // SAFETY: every event is live for the whole loop.
        let first = unsafe { WaitForMultipleObjects(&events, false, 250) };
        let at = started.elapsed().as_secs_f32();
        for (index, method) in methods.iter_mut().enumerate() {
            // The one the wait returned was reset by it; the others are
            // asked without waiting, which resets them too.
            let fired = first.0 == WAIT_OBJECT_0.0 + index as u32
                // SAFETY: a live event, not waited on.
                || unsafe { WaitForSingleObject(method.event, 0) } == WAIT_OBJECT_0;
            if fired {
                method.fired += 1;
                log(&format!("methods: +{at:.1}s FIRED {}", method.name));
                method.arm();
            }
        }
        let current_writer = presence_writer::running_pid(&exe);
        match (writer, current_writer) {
            (None, Some(pid)) => log(&format!("methods: +{at:.1}s WRITER STARTED pid {pid}")),
            (Some(pid), None) => log(&format!("methods: +{at:.1}s WRITER EXITED pid {pid}")),
            _ => {}
        }
        writer = current_writer;
        let after = read_entries(GAME_LIST);
        diff(&before, &after, &format!("+{at:.1}s CHANGE"));
        before = after;
    }
    for method in &methods {
        log(&format!(
            "methods: {} fired {} times",
            method.name, method.fired
        ));
        // SAFETY: the event created above, closed once, after the loop.
        unsafe { _ = CloseHandle(method.event) };
    }
    Ok(())
}

// ------------------------------------------------ what the watcher holds --

/// This process's private bytes and handle count, now.
fn footprint() -> (f64, u32) {
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    let mut memory = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let mut handles = 0u32;
    // SAFETY: the pseudo handle needs no closing; the counters struct is
    // passed with its own size, which bounds the write; `handles` is a local.
    unsafe {
        let _ = K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut memory).cast::<PROCESS_MEMORY_COUNTERS>(),
            memory.cb,
        );
        let _ = GetProcessHandleCount(GetCurrentProcess(), &mut handles);
    }
    (memory.PrivateUsage as f64 / (1024.0 * 1024.0), handles)
}

/// Run, one step at a time, what the watcher does at start and at every
/// idle look, and say what each step leaves behind in private memory and
/// handles -- a step repeated ten times that leaves more each time is a
/// leak, one that leaves the same is a cost.
fn cmd_footprint() -> windows::core::Result<()> {
    use game_mode_executor::detect::hand_made;
    use game_mode_executor::detect::known_games::KnownGames;
    use game_mode_executor::detect::process::Snapshot;
    use game_mode_executor::sensor::{self, Sensor};

    let say = |step: &str| {
        let (private, handles) = footprint();
        println!("{private:>7.2} MB private  {handles:>5} handles  after {step}");
    };
    say("start");
    let entries = hand_made::load().unwrap_or_default();
    say("reading the games marked by hand once");
    for _ in 0..10 {
        let _ = hand_made::load();
    }
    say("reading them ten times more");
    let _ = hand_made::covered_by_microsoft(&entries);
    say("comparing them with Microsoft's list once");
    for _ in 0..10 {
        let _ = hand_made::covered_by_microsoft(&entries);
    }
    say("comparing ten times more");
    for _ in 0..10 {
        let _ = KnownGames::load();
    }
    say("loading the Known Game List ten times, for naming");
    for _ in 0..10 {
        let _ = Snapshot::take();
    }
    say("ten full process snapshots");
    let Ok(sensor) = sensor::Windows::new() else {
        println!("no sensor: the Game Bar is not registered here");
        return Ok(());
    };
    say("building the sensor");
    let _ = sensor.sighting();
    say("its first look");
    for _ in 0..100 {
        let _ = sensor.sighting();
    }
    say("a hundred looks more");
    // What a session's start does, which 0.2.0 did too: every process asked
    // for its path and package to name the game, then the GPU counters for
    // the refinement.
    let _ = sensor.candidates();
    say("naming a game once: every process asked its path and package");
    for _ in 0..3 {
        let _ = sensor.candidates();
    }
    say("naming three times more");
    let _ = sensor.rendering_load(std::time::Duration::from_millis(200));
    say("reading the GPU counters once, for the refinement");
    for _ in 0..3 {
        let _ = sensor.rendering_load(std::time::Duration::from_millis(200));
    }
    say("reading them three times more");
    // What a click on *Edit configuration* costs: the menu opens the file
    // through the shell. A hidden `cmd /c exit 0` takes the same road
    // without putting anything on screen.
    let shell = |times: usize| {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
        use windows::core::w;
        for _ in 0..times {
            // SAFETY: every string is a NUL-terminated literal.
            unsafe {
                ShellExecuteW(
                    None,
                    w!("open"),
                    w!("cmd.exe"),
                    w!("/c exit 0"),
                    None,
                    SW_HIDE,
                )
            };
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    };
    shell(1);
    say("opening a program through the shell once, as the menu does");
    shell(3);
    say("three times more");
    Ok(())
}

// ------------------------------------------------------- the GPU reader --

/// The rendering load the refinement reads, every `ms` milliseconds, for
/// `rounds` rounds: the processes with any, busiest first. Run beside
/// `typeperf "\GPU Engine(*engtype_3D)\Utilization Percentage"` to check
/// the reader against Windows' own tool.
fn cmd_gpu_load(ms: u64, rounds: u32) -> windows::core::Result<()> {
    use game_mode_executor::detect::gpu;
    for _ in 0..rounds {
        match gpu::rendering_load(std::time::Duration::from_millis(ms)) {
            Ok(load) => {
                let mut busiest: Vec<_> = load.into_iter().collect();
                busiest.sort_by(|a, b| b.1.total_cmp(&a.1));
                let line: Vec<String> = busiest
                    .iter()
                    .take(5)
                    .map(|(pid, share)| format!("{pid}={share:.2}%"))
                    .collect();
                println!("{} {}", timestamp(), line.join("  "));
            }
            Err(error) => println!("{} cannot read: {error:#}", timestamp()),
        }
    }
    Ok(())
}

/// What one click of the menu leaves in this process, sampled for five
/// minutes: `open`, the shell road *Edit configuration* takes, here to a
/// hidden `cmd /c exit 0` so nothing shows; `check`, *Check for updates*
/// over the network; `spawn`, a plain CreateProcess, for comparison.
fn cmd_menu_cost(click: &str) -> windows::core::Result<()> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::w;

    let threads = || {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        };
        let me = std::process::id();
        let mut count = 0;
        // SAFETY: the snapshot is closed below; the entry carries its size.
        unsafe {
            let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
                return 0;
            };
            let mut entry = THREADENTRY32 {
                dwSize: size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut more = Thread32First(snapshot, &mut entry).is_ok();
            while more {
                if entry.th32OwnerProcessID == me {
                    count += 1;
                }
                more = Thread32Next(snapshot, &mut entry).is_ok();
            }
            let _ = windows::Win32::Foundation::CloseHandle(snapshot);
        }
        count
    };
    let say = |step: &str| {
        let (private, handles) = footprint();
        println!(
            "{private:>7.2} MB private  {handles:>5} handles  {:>3} threads  {step}",
            threads()
        );
    };
    say("before");
    match click {
        "menu" | "menu-dark" => {
            show_menus(click == "menu-dark", &say);
            return Ok(());
        }
        "open" => {
            // SAFETY: every string is a NUL-terminated literal.
            unsafe {
                ShellExecuteW(
                    None,
                    w!("open"),
                    w!("cmd.exe"),
                    w!("/c exit 0"),
                    None,
                    SW_HIDE,
                )
            };
        }
        "check" => {
            use game_mode_executor::update;
            if let Ok(context) = update::Context::of_this_process(None, None) {
                let verdict = update::check_now(&context);
                println!("check: {:?}", verdict.map(|_| "answered"));
            }
        }
        "spawn" => {
            let _ = std::process::Command::new("cmd.exe")
                .args(["/c", "exit", "0"])
                .status();
        }
        other => {
            eprintln!(
                "usage: presence-probe menu-cost <open|check|spawn|menu|menu-dark>, not `{other}`"
            );
            std::process::exit(2);
        }
    }
    for (wait, at) in [(1, 1), (4, 5), (25, 30), (90, 120), (180, 300)] {
        std::thread::sleep(std::time::Duration::from_secs(wait));
        say(&format!("{click}, {at} s after"));
    }
    Ok(())
}

/// The tray's menu, built the tray's way on a hidden window of this process
/// and shown three times, each closed by a timer after a second -- with or
/// without asking uxtheme for dark menus first, as the tray does at start.
/// It is on screen for that second.
fn show_menus(dark: bool, say: &dyn Fn(&str)) {
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
        DestroyWindow, EndMenu, GetCursorPos, KillTimer, MF_DISABLED, MF_GRAYED, MF_SEPARATOR,
        MF_STRING, PostMessageW, RegisterClassW, SetForegroundWindow, SetTimer, TPM_NONOTIFY,
        TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WINDOW_EX_STYLE, WM_NULL, WNDCLASSW,
        WS_POPUP,
    };
    use windows::core::{PCSTR, w};

    extern "system" fn procedure(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        // SAFETY: the arguments are the ones Windows passed in.
        unsafe { DefWindowProcW(window, message, w, l) }
    }
    extern "system" fn close(_: HWND, _: u32, _: usize, _: u32) {
        // SAFETY: no arguments; ends whatever menu this thread shows.
        let _ = unsafe { EndMenu() };
    }

    if dark {
        // The tray's call, as `tray::dark` makes it: ordinals 135 and 136.
        // SAFETY: the name is a literal; the pointers resolved by ordinal are
        // called with the signatures the tray uses on this build.
        unsafe {
            if let Ok(uxtheme) = LoadLibraryW(w!("uxtheme.dll")) {
                if let Some(set) = GetProcAddress(uxtheme, PCSTR(135 as *const u8)) {
                    let set: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set);
                    set(1);
                }
                if let Some(flush) = GetProcAddress(uxtheme, PCSTR(136 as *const u8)) {
                    let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
                    flush();
                }
            }
        }
        say("after SetPreferredAppMode");
    }

    // SAFETY: the class name is a literal, the procedure lives for the
    // process, and the window is destroyed at the end.
    let window = unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: w!("presence-probe-menu"),
            ..Default::default()
        };
        RegisterClassW(&class);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("presence-probe-menu"),
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
    };
    let Ok(window) = window else {
        println!("no window");
        return;
    };
    say("after creating a hidden window");

    for round in 1..=3 {
        // SAFETY: as the tray's `show_menu`: the strings are literals, the menu
        // is destroyed on every path, the timer is killed after the menu
        // closed, and nothing is borrowed across the modal loop.
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            let _ = AppendMenuW(
                menu,
                MF_STRING | MF_DISABLED | MF_GRAYED,
                0,
                w!("GameModeExecutor - no game detected"),
            );
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(menu, MF_STRING, 1, w!("Edit configuration"));
            let _ = AppendMenuW(menu, MF_STRING, 2, w!("Open log"));
            let _ = AppendMenuW(menu, MF_STRING, 3, w!("Documentation"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(menu, MF_STRING, 4, w!("Check for updates"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(menu, MF_STRING, 5, w!("Quit"));
            let mut at = POINT::default();
            let _ = GetCursorPos(&mut at);
            let timer = SetTimer(None, 0, 1000, Some(close));
            let _ = SetForegroundWindow(window);
            let _ = TrackPopupMenuEx(
                menu,
                TPM_RIGHTBUTTON.0 | TPM_RETURNCMD.0 | TPM_NONOTIFY.0,
                at.x,
                at.y,
                window,
                None,
            );
            let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = KillTimer(None, timer);
            let _ = DestroyMenu(menu);
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        say(&format!("after the menu, shown and closed, {round}"));
    }
    // SAFETY: created above, destroyed once.
    let _ = unsafe { DestroyWindow(window) };
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
        Some("footprint") => cmd_footprint(),
        Some("gpu-load") => cmd_gpu_load(
            std::env::args()
                .nth(2)
                .and_then(|value| value.parse().ok())
                .unwrap_or(1000),
            std::env::args()
                .nth(3)
                .and_then(|value| value.parse().ok())
                .unwrap_or(5),
        ),
        Some("menu-cost") => cmd_menu_cost(std::env::args().nth(2).as_deref().unwrap_or("")),
        Some("watch-methods") => match std::env::args().nth(3) {
            Some(sid) => cmd_watch_methods(seconds(), &sid),
            None => {
                eprintln!("usage: presence-probe watch-methods <seconds> <sid>");
                std::process::exit(2);
            }
        },
        Some("microsoft-list") => {
            use game_mode_executor::detect::microsoft_list;
            let Some(path) = microsoft_list::path() else {
                eprintln!("no local profile");
                std::process::exit(1);
            };
            let list = match microsoft_list::read(&path) {
                Ok(list) => list,
                Err(error) => {
                    eprintln!("cannot read {}: {error}", path.display());
                    std::process::exit(1);
                }
            };
            println!(
                "{}: {} bytes, revision {}",
                path.display(),
                list.len(),
                list.get(8..12)
                    .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                    .unwrap_or_default()
            );
            let list = microsoft_list::List::new(list);
            for exe in std::env::args().skip(2) {
                let answer = if list.covers(&exe) {
                    "listed"
                } else {
                    "not listed"
                };
                println!("  {answer:<10} {exe}");
            }
            Ok(())
        }
        Some("cost") => cmd_cost(
            std::env::args()
                .nth(2)
                .and_then(|value| value.parse().ok())
                .unwrap_or(200),
        ),
        Some(other) => {
            eprintln!("unknown command `{other}`");
            eprintln!(
                "usage: presence-probe [status|watch [seconds]|watch-games [seconds]|cost [rounds]|activate]"
            );
            std::process::exit(2);
        }
    }
}
