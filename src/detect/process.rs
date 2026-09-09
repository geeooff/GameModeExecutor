//! Process enumeration and the process-name detector.

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    /// Executable file name, e.g. `cs2.exe`.
    pub name: String,
}

/// Every process visible to the current user, taken once per tick and shared
/// by all detectors.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub processes: Vec<ProcessInfo>,
}

impl Snapshot {
    pub fn take() -> Result<Self> {
        let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if handle == INVALID_HANDLE_VALUE {
            bail!("CreateToolhelp32Snapshot failed (error {})", unsafe {
                GetLastError()
            });
        }

        let mut processes = Vec::with_capacity(256);
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;

        let mut ok = unsafe { Process32FirstW(handle, &mut entry) };
        while ok != 0 {
            processes.push(ProcessInfo {
                pid: entry.th32ProcessID,
                name: from_wide_nul(&entry.szExeFile),
            });
            ok = unsafe { Process32NextW(handle, &mut entry) };
        }
        unsafe { CloseHandle(handle) };

        Ok(Self { processes })
    }

    pub fn by_pid(&self, pid: u32) -> Option<&ProcessInfo> {
        self.processes.iter().find(|process| process.pid == pid)
    }
}

/// Best-effort full image path; returns `None` for processes we may not open.
pub fn full_path(pid: u32) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = [0u16; 32768];
    let mut size = buffer.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size)
    };
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..size as usize]))
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
}
