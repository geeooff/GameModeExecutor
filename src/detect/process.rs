//! Process enumeration and identity.

use anyhow::{Context, Result};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

/// A borrowed Win32 handle closed on drop.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { _ = CloseHandle(self.0) };
    }
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    /// Executable file name, e.g. `cs2.exe`.
    pub name: String,
}

/// Every process visible to the current user, taken in one pass.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub processes: Vec<ProcessInfo>,
}

impl Snapshot {
    pub fn take() -> Result<Self> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
            .context("CreateToolhelp32Snapshot failed")?;
        let snapshot = OwnedHandle(snapshot);

        let mut processes = Vec::with_capacity(256);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        let mut ok = unsafe { Process32FirstW(snapshot.0, &mut entry) };
        while ok.is_ok() {
            processes.push(ProcessInfo {
                pid: entry.th32ProcessID,
                name: from_wide_nul(&entry.szExeFile),
            });
            ok = unsafe { Process32NextW(snapshot.0, &mut entry) };
        }
        Ok(Self { processes })
    }

    pub fn by_pid(&self, pid: u32) -> Option<&ProcessInfo> {
        self.processes.iter().find(|process| process.pid == pid)
    }
}

/// Open a process for the read-only queries below. Fails for processes the
/// current user may not touch, which is expected and not an error here.
fn open_for_query(pid: u32) -> Option<OwnedHandle> {
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .ok()
        .map(OwnedHandle)
}

/// Best-effort full image path.
pub fn full_path(pid: u32) -> Option<String> {
    let handle = open_for_query(pid)?;
    let mut buffer = [0u16; 32768];
    let mut size = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&buffer[..size as usize]))
}

/// Package family name of a packaged (Store or Game Pass) process, e.g.
/// `BethesdaSoftworks.ProjectGold_3275kfvn8vcwc`. `None` for ordinary Win32
/// processes, which is the common case.
pub fn package_family_name(pid: u32) -> Option<String> {
    let handle = open_for_query(pid)?;
    let mut length = 0u32;
    // First call sizes the buffer; it fails with ERROR_INSUFFICIENT_BUFFER for
    // a packaged process and with APPMODEL_ERROR_NO_PACKAGE otherwise.
    unsafe { GetPackageFamilyName(handle.0, &mut length, None) }
        .ok()
        .err()?;
    if length == 0 {
        return None;
    }
    let mut buffer = vec![0u16; length as usize];
    unsafe { GetPackageFamilyName(handle.0, &mut length, Some(PWSTR(buffer.as_mut_ptr()))) }
        .ok()
        .ok()?;
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}

fn from_wide_nul(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_contains_the_test_process() {
        let snapshot = Snapshot::take().unwrap();
        assert!(snapshot.by_pid(std::process::id()).is_some());
    }

    #[test]
    fn full_path_of_the_test_process_is_readable() {
        let path = full_path(std::process::id()).expect("own image path");
        assert!(path.to_ascii_lowercase().ends_with(".exe"), "got {path}");
    }

    #[test]
    fn an_unpackaged_process_has_no_package_family_name() {
        // The test binary is a plain Win32 executable.
        assert_eq!(package_family_name(std::process::id()), None);
    }
}
