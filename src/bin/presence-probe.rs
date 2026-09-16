//! Spike: a Game Bar Presence Writer that only observes.
//!
//! Windows exposes one documented extension point that says when *Windows
//! itself* considers a game to have gained focus, lost focus, or closed: the
//! Game Bar Presence Writer.
//! <https://learn.microsoft.com/en-us/windows/win32/devnotes/gamebar-presencewriter>
//!
//! Registering a custom one turned out to be impossible: the registration key
//! is owned by `NT SERVICE\TrustedInstaller`, and neither Administrators nor
//! SYSTEM can write it. `install` is kept for the record and fails with access
//! denied; `serve` is what it would have run.
//!
//! What works is observing the registered writer instead. Which executable
//! that is comes from the registry, never from a hard-coded name, so a machine
//! where something else owns the registration is probed correctly.
//!
//! Usage:
//!   presence-probe status      show the current registration and log path
//!   presence-probe install     point the registration at this exe (needs admin)
//!   presence-probe uninstall   restore the original registration (needs admin)
//!   presence-probe watch [s]   log when Windows' own presence writer runs
//!   presence-probe activate    activate the class ourselves and time it
//!   presence-probe serve       run as the COM server (what Windows invokes)

// The interface below mirrors the WinRT declaration, PascalCase members and all.
#![allow(non_snake_case)]

#[cfg(not(windows))]
compile_error!("GameModeExecutor only targets Windows");

use std::ffi::c_void;
use std::io::Write;
use std::path::PathBuf;

use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_SAM_FLAGS, REG_SZ, RegCloseKey,
    RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::WinRT::{
    IActivationFactory, IActivationFactory_Impl, RO_INIT_MULTITHREADED, RoInitialize,
    RoRegisterActivationFactories,
};
use windows::core::{HRESULT, HSTRING, IInspectable, Interface, OutRef, PCWSTR, Ref, implement};

use game_mode_executor::detect::presence_writer;

/// The runtime class Windows activates when game presence changes.
const CLASS_ID: &str = presence_writer::CLASS_ID;

/// Where the server executable is looked up.
const SERVER_KEY: &str = r"SOFTWARE\Microsoft\WindowsRuntime\Server\Windows.Gaming.GameBar.Internal.PresenceWriterServer";
const EXE_PATH_VALUE: &str = "ExePath";
/// Our copy of the original `ExePath`, so `uninstall` can put it back.
const BACKUP_VALUE: &str = "ExePath.GameModeExecutorBackup";

// -- The interface, transcribed from the MIDL in the Microsoft devnotes page --

/// `GameNotificationEvent` from the documented IDL.
fn event_name(event: i32) -> &'static str {
    match event {
        0 => "None",
        1 => "GotFocus",
        2 => "LostFocus",
        3 => "AppClose",
        _ => "unknown",
    }
}

/// `AppIdType` from the documented IDL.
fn app_id_type_name(app_id_type: i32) -> &'static str {
    match app_id_type {
        0 => "Aumid",
        1 => "TitleId",
        _ => "unknown",
    }
}

// `IPresenceWriter` derives from `IInspectable`, which the `#[interface]`
// attribute cannot express, so it is declared the way the `windows` crate
// declares its own WinRT interfaces.
windows_core::imp::define_interface!(
    IPresenceWriter,
    IPresenceWriter_Vtbl,
    0x782674d9_5cbb_4fca_ad72_d9ac5f7ae963
);
windows_core::imp::interface_hierarchy!(
    IPresenceWriter,
    windows_core::IUnknown,
    windows_core::IInspectable
);

#[repr(C)]
#[doc(hidden)]
#[allow(non_camel_case_types)]
pub struct IPresenceWriter_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub UpdatePresence:
        unsafe extern "system" fn(*mut c_void, u64, i32, *mut c_void, i32) -> HRESULT,
}

#[allow(non_camel_case_types)]
pub trait IPresenceWriter_Impl: windows_core::IUnknownImpl {
    /// `hwnd` is the game window, `app_id` an AUMID or an Xbox Live title id.
    /// The HSTRING is borrowed: it stays owned by the caller.
    fn UpdatePresence(
        &self,
        hwnd: u64,
        event: i32,
        app_id: &HSTRING,
        app_id_type: i32,
    ) -> windows_core::Result<()>;
}

impl IPresenceWriter_Vtbl {
    pub const fn new<Identity: IPresenceWriter_Impl, const OFFSET: isize>() -> Self {
        unsafe extern "system" fn UpdatePresence<
            Identity: IPresenceWriter_Impl,
            const OFFSET: isize,
        >(
            this: *mut c_void,
            hwnd: u64,
            event: i32,
            app_id: *mut c_void,
            app_id_type: i32,
        ) -> HRESULT {
            // SAFETY: this is the vtable thunk shape the `windows` crate's
            // `implement` macro generates. `this` is the interface pointer
            // COM handed us, `OFFSET` is the interface's position inside the
            // implementing object, and `app_id` is an HSTRING the caller owns
            // for the duration of the call, so borrowing it is sound.
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                IPresenceWriter_Impl::UpdatePresence(
                    this,
                    hwnd,
                    event,
                    core::mem::transmute::<&*mut c_void, &HSTRING>(&app_id),
                    app_id_type,
                )
                .into()
            }
        }
        Self {
            base__: windows_core::IInspectable_Vtbl::new::<Identity, IPresenceWriter, OFFSET>(),
            UpdatePresence: UpdatePresence::<Identity, OFFSET>,
        }
    }

    pub fn matches(iid: &windows_core::GUID) -> bool {
        iid == &<IPresenceWriter as Interface>::IID
    }
}

impl windows_core::RuntimeName for IPresenceWriter {
    const NAME: &'static str = CLASS_ID;
}

#[implement(IPresenceWriter)]
struct PresenceWriter;

impl IPresenceWriter_Impl for PresenceWriter_Impl {
    fn UpdatePresence(
        &self,
        hwnd: u64,
        event: i32,
        app_id: &HSTRING,
        app_id_type: i32,
    ) -> windows_core::Result<()> {
        let app_id = app_id.to_string();
        log(&format!(
            "UpdatePresence event={} ({event}) app_id={app_id:?} app_id_type={} ({app_id_type}) hwnd=0x{hwnd:x}",
            event_name(event),
            app_id_type_name(app_id_type),
        ));
        Ok(())
    }
}

#[implement(IActivationFactory)]
struct PresenceWriterFactory;

impl IActivationFactory_Impl for PresenceWriterFactory_Impl {
    fn ActivateInstance(&self) -> windows::core::Result<IInspectable> {
        log("ActivateInstance: handing out a PresenceWriter");
        let writer: IPresenceWriter = PresenceWriter.into();
        writer.cast()
    }
}

unsafe extern "system" fn get_activation_factory(
    class_id: Ref<HSTRING>,
    factory: OutRef<IActivationFactory>,
) -> HRESULT {
    let requested = class_id
        .as_ref()
        .map(HSTRING::to_string)
        .unwrap_or_default();
    log(&format!("activation factory requested for {requested:?}"));
    let instance: IActivationFactory = PresenceWriterFactory.into();
    match factory.write(Some(instance)) {
        Ok(()) => HRESULT(0),
        Err(error) => error.code(),
    }
}

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

// --------------------------------------------------------------- registry --

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

struct RegKey(HKEY);

impl RegKey {
    fn open(access: REG_SAM_FLAGS) -> windows::core::Result<Self> {
        let subkey = wide(SERVER_KEY);
        let mut key = HKEY::default();
        // SAFETY: `subkey` is NUL-terminated and outlives the call, `key` is
        // a valid out pointer, and the handle is closed on drop.
        unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                PCWSTR(subkey.as_ptr()),
                None,
                access,
                &mut key,
            )
            .ok()?;
        }
        Ok(Self(key))
    }

    fn read(&self, name: &str) -> Option<String> {
        let name = wide(name);
        let mut size = 0u32;
        // SAFETY: the first call asks for the size only; the second is given a
        // buffer of exactly that many bytes, so the write is bounded.
        unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                None,
                None,
                Some(&mut size),
            )
            .ok()
            .ok()?;
            let mut buffer = vec![0u8; size as usize];
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                None,
                Some(buffer.as_mut_ptr()),
                Some(&mut size),
            )
            .ok()
            .ok()?;
            let units: Vec<u16> = buffer
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&pair| u16::from_le_bytes(pair))
                .take_while(|&unit| unit != 0)
                .collect();
            Some(String::from_utf16_lossy(&units))
        }
    }

    fn write(&self, name: &str, value: &str) -> windows::core::Result<()> {
        let name = wide(name);
        let data = wide(value);
        // SAFETY: a `[u16]` viewed as twice as many bytes, within the same
        // allocation, for the duration of the borrow.
        let bytes: &[u8] =
            unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2) };
        // SAFETY: `name` is NUL-terminated and `bytes` is the NUL-terminated
        // UTF-16 value; both outlive the call.
        unsafe { RegSetValueExW(self.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes)).ok() }
    }

    fn delete(&self, name: &str) -> windows::core::Result<()> {
        let name = wide(name);
        // SAFETY: `name` is NUL-terminated and the key is open.
        unsafe { RegDeleteValueW(self.0, PCWSTR(name.as_ptr())).ok() }
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: the handle came from `RegOpenKeyExW` and is closed once.
        unsafe { _ = RegCloseKey(self.0) };
    }
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
    let Some(pid) = game_mode_executor::detect::fullscreen::foreground_pid() else {
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
            match object.cast::<IPresenceWriter>() {
                Ok(_) => log("activate: object exposes IPresenceWriter"),
                Err(error) => log(&format!("activate: not an IPresenceWriter: {error}")),
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
    let key = RegKey::open(KEY_READ)?;
    let current = key.read(EXE_PATH_VALUE).unwrap_or_default();
    let backup = key.read(BACKUP_VALUE);
    let ours = std::env::current_exe().unwrap_or_default();

    println!("Registration key : HKLM\\{SERVER_KEY}");
    println!("  {EXE_PATH_VALUE:<34} = {current}");
    match &backup {
        Some(path) => println!("  {BACKUP_VALUE:<34} = {path}"),
        None => println!("  {BACKUP_VALUE:<34} = (absent, nothing to restore)"),
    }
    println!(
        "  Microsoft default  : {}",
        if presence_writer::is_microsoft_default(std::path::Path::new(&current)) {
            "yes"
        } else {
            "NO - something else owns the registration"
        }
    );
    match presence_writer::running_pid(std::path::Path::new(&current)) {
        Some(pid) => println!("Writer running    : yes (pid {pid})"),
        None => println!("Writer running    : no"),
    }
    println!("This executable   : {}", ours.display());
    println!(
        "Probe installed   : {}",
        if current.eq_ignore_ascii_case(&ours.to_string_lossy()) {
            "yes"
        } else {
            "no"
        }
    );
    println!("Log file          : {}", log_path().display());
    Ok(())
}

fn cmd_install() -> windows::core::Result<()> {
    let exe = std::env::current_exe().expect("current exe");
    let exe = exe.to_string_lossy().to_string();
    let key = RegKey::open(KEY_READ | KEY_SET_VALUE)?;

    let current = key.read(EXE_PATH_VALUE).unwrap_or_default();
    if current.eq_ignore_ascii_case(&exe) {
        println!("Already installed, nothing to do.");
        return Ok(());
    }
    // Only ever back up a value that is not already ours, so installing twice
    // cannot lose the original path.
    if key.read(BACKUP_VALUE).is_none() {
        key.write(BACKUP_VALUE, &current)?;
        println!("Backed up original ExePath: {current}");
    }
    key.write(EXE_PATH_VALUE, &exe)?;
    println!("ExePath now points at: {exe}");
    println!();
    println!("Xbox Live presence is no longer written while this is installed.");
    println!("Launch a game, then read: {}", log_path().display());
    println!("Undo with: presence-probe uninstall");
    Ok(())
}

fn cmd_uninstall() -> windows::core::Result<()> {
    let key = RegKey::open(KEY_READ | KEY_SET_VALUE)?;
    let Some(original) = key.read(BACKUP_VALUE) else {
        println!("No backup value found; leaving the registration untouched.");
        println!("The Windows default is C:\\Windows\\System32\\GameBarPresenceWriter.exe");
        return Ok(());
    };
    key.write(EXE_PATH_VALUE, &original)?;
    key.delete(BACKUP_VALUE)?;
    println!("Restored ExePath: {original}");
    Ok(())
}

fn cmd_serve() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    log(&format!("serve: started, argv={args:?}"));

    // SAFETY: initialises the Windows Runtime for this thread; no pointers.
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };

    let class_ids = [HSTRING::from(CLASS_ID)];
    let callbacks = [Some(
        get_activation_factory
            as unsafe extern "system" fn(Ref<HSTRING>, OutRef<IActivationFactory>) -> HRESULT,
    )];
    // SAFETY: both arrays have one element and outlive the registration --
    // the function never returns, so they live for the process.
    unsafe {
        RoRegisterActivationFactories(
            class_ids.as_ptr(),
            callbacks.as_ptr(),
            class_ids.len() as u32,
        )?
    };
    log(&format!(
        "serve: registered {CLASS_ID}, waiting for presence events"
    ));

    // Nothing else to do: Windows calls in when a game changes state.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn main() -> windows::core::Result<()> {
    // Windows launches the server with no arguments, so that is the default.
    match std::env::args().nth(1).as_deref() {
        Some("status") => cmd_status(),
        Some("watch") => {
            let seconds = std::env::args()
                .nth(2)
                .and_then(|value| value.parse().ok())
                .unwrap_or(600);
            cmd_watch(seconds)
        }
        Some("activate") => cmd_activate(5, 60),
        Some("install") => cmd_install(),
        Some("uninstall") => cmd_uninstall(),
        None | Some("serve") => cmd_serve(),
        Some(other) => {
            eprintln!("unknown command `{other}`");
            eprintln!(
                "usage: presence-probe [status|watch [seconds]|activate|install|uninstall|serve]"
            );
            std::process::exit(2);
        }
    }
}
