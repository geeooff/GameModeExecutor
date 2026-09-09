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
        Some("install") => cmd_install(),
        Some("uninstall") => cmd_uninstall(),
        None | Some("serve") => cmd_serve(),
        Some(other) => {
            eprintln!("unknown command `{other}`");
            eprintln!("usage: presence-probe [status|install|uninstall|serve]");
            std::process::exit(2);
        }
    }
}
