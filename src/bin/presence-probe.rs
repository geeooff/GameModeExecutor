//! Spike: a Game Bar Presence Writer that only observes.
//!
//! Windows exposes one documented extension point that says when *Windows
//! itself* considers a game to have gained focus, lost focus, or closed: the
//! Game Bar Presence Writer.
//! <https://learn.microsoft.com/en-us/windows/win32/devnotes/gamebar-presencewriter>
//!
//! A custom implementation is an out-of-proc WinRT server registered under
//! `HKLM\SOFTWARE\Microsoft\WindowsRuntime\Server\Windows.Gaming.GameBar.Internal.PresenceWriterServer\ExePath`.
//! Registering one REPLACES the shipped `GameBarPresenceWriter.exe`, which is
//! what sets Xbox Live presence, so this probe backs the original value up and
//! can restore it.
//!
//! The probe answers what the documentation does not: which events Windows
//! actually sends, with which identifiers, and when.
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

/// The runtime class Windows activates when game presence changes.
const CLASS_ID: &str = "Windows.Gaming.GameBar.PresenceServer.Internal.PresenceWriter";

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
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .take_while(|&unit| unit != 0)
                .collect();
            Some(String::from_utf16_lossy(&units))
        }
    }

    fn write(&self, name: &str, value: &str) -> windows::core::Result<()> {
        let name = wide(name);
        let data = wide(value);
        let bytes: &[u8] =
            unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2) };
        unsafe { RegSetValueExW(self.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes)).ok() }
    }

    fn delete(&self, name: &str) -> windows::core::Result<()> {
        let name = wide(name);
        unsafe { RegDeleteValueW(self.0, PCWSTR(name.as_ptr())).ok() }
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        unsafe { _ = RegCloseKey(self.0) };
    }
}

// ------------------------------------------------- observing the default --

/// The executable Windows ships as the presence writer.
const DEFAULT_WRITER: &str = "gamebarpresencewriter.exe";

/// PID of a running process with this file name, if any.
fn find_process(file_name: &str) -> Option<u32> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = None;
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok.is_ok() {
        let end = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if name.eq_ignore_ascii_case(file_name) {
            found = Some(entry.th32ProcessID);
            break;
        }
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    unsafe { _ = windows::Win32::Foundation::CloseHandle(snapshot) };
    found
}

/// File name of the process owning the foreground window, to correlate a
/// presence writer launch with whatever the user was doing.
fn foreground_process_name() -> String {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return "(none)".to_owned();
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    if pid == 0 {
        return "(none)".to_owned();
    }
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return "(unknown)".to_owned();
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut name = "(unknown)".to_owned();
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok.is_ok() {
        if entry.th32ProcessID == pid {
            let end = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            name = String::from_utf16_lossy(&entry.szExeFile[..end]);
            break;
        }
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    unsafe { _ = windows::Win32::Foundation::CloseHandle(snapshot) };
    format!("{name} (pid {pid})")
}

/// Purely passive: log when Windows' own presence writer comes and goes.
/// Nothing is modified, nothing needs admin. Run it, then play a game.
fn cmd_watch(seconds: u64) -> windows::core::Result<()> {
    let mut last = find_process(DEFAULT_WRITER);
    log(&format!(
        "watch: {DEFAULT_WRITER} is {} at start, watching for {seconds}s",
        match last {
            Some(pid) => format!("RUNNING (pid {pid})"),
            None => "not running".to_owned(),
        }
    ));

    // 100ms so a short-lived launch is not missed. This is a measurement
    // tool, not the shipping detector: it costs a process snapshot per tick.
    let started = std::time::Instant::now();
    while started.elapsed().as_secs() < seconds {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let current = find_process(DEFAULT_WRITER);
        match (last, current) {
            (None, Some(pid)) => log(&format!(
                "watch: STARTED pid {pid} at +{:.1}s, foreground = {}",
                started.elapsed().as_secs_f32(),
                foreground_process_name()
            )),
            (Some(old), None) => log(&format!(
                "watch: EXITED pid {old} at +{:.1}s, foreground = {}",
                started.elapsed().as_secs_f32(),
                foreground_process_name()
            )),
            (Some(old), Some(new)) if old != new => log(&format!(
                "watch: RESTARTED pid {old} -> {new} at +{:.1}s",
                started.elapsed().as_secs_f32()
            )),
            _ => {}
        }
        last = current;
    }
    log("watch: done");
    Ok(())
}

/// Activate the runtime class ourselves, to find out whether Windows starts
/// the writer on demand and how long it lingers once nobody holds it.
fn cmd_activate(hold: u64, linger: u64) -> windows::core::Result<()> {
    use windows::Win32::System::WinRT::RoActivateInstance;

    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };

    let before = find_process(DEFAULT_WRITER);
    log(&format!(
        "activate: {DEFAULT_WRITER} before = {}",
        match before {
            Some(pid) => format!("running (pid {pid})"),
            None => "not running".to_owned(),
        }
    ));

    let started = std::time::Instant::now();
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
        if let Some(pid) = find_process(DEFAULT_WRITER)
            && before != Some(pid)
        {
            spawned = Some(pid);
            log(&format!(
                "activate: {DEFAULT_WRITER} appeared as pid {pid} after {:.0}ms",
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
        if find_process(DEFAULT_WRITER).is_none() {
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

    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };

    let class_ids = [HSTRING::from(CLASS_ID)];
    let callbacks = [Some(
        get_activation_factory
            as unsafe extern "system" fn(Ref<HSTRING>, OutRef<IActivationFactory>) -> HRESULT,
    )];
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
