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
//! presence-probe status        the current registration and the log path
//! presence-probe watch [secs]  log when Windows' own presence writer runs
//! presence-probe activate      activate the class ourselves and time it
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

fn main() -> windows::core::Result<()> {
    match std::env::args().nth(1).as_deref() {
        None | Some("status") => cmd_status(),
        Some("watch") => {
            let seconds = std::env::args()
                .nth(2)
                .and_then(|value| value.parse().ok())
                .unwrap_or(600);
            cmd_watch(seconds)
        }
        Some("activate") => cmd_activate(5, 60),
        Some(other) => {
            eprintln!("unknown command `{other}`");
            eprintln!("usage: presence-probe [status|watch [seconds]|activate]");
            std::process::exit(2);
        }
    }
}
