//! Process enumeration and the process-name detector.

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

use super::GameSignal;

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

/// Normalise a configured or observed executable name for comparison:
/// lowercase, without the `.exe` suffix.
pub fn normalize_name(name: &str) -> String {
    let lowered = name.trim().to_ascii_lowercase();
    lowered
        .strip_suffix(".exe")
        .map(str::to_owned)
        .unwrap_or(lowered)
}

/// Matches when one of the configured executable names is running.
#[derive(Debug)]
pub struct ProcessDetector {
    names: Vec<String>,
}

impl ProcessDetector {
    pub fn new(names: &[String]) -> Self {
        Self {
            names: names.iter().map(|name| normalize_name(name)).collect(),
        }
    }

    pub fn detect(&self, snapshot: &Snapshot) -> Option<GameSignal> {
        let process = snapshot
            .processes
            .iter()
            .find(|process| self.names.contains(&normalize_name(&process.name)))?;
        Some(GameSignal {
            source: "process",
            process_name: Some(process.name.clone()),
            process_id: Some(process.pid),
            process_path: full_path(process.pid),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_normalized() {
        assert_eq!(normalize_name("CS2.EXE"), "cs2");
        assert_eq!(normalize_name("  EldenRing  "), "eldenring");
        assert_eq!(normalize_name("cs2"), "cs2");
    }

    #[test]
    fn snapshot_contains_the_test_process() {
        let snapshot = Snapshot::take().unwrap();
        assert!(snapshot.by_pid(std::process::id()).is_some());
    }

    #[test]
    fn detector_matches_the_test_process() {
        let snapshot = Snapshot::take().unwrap();
        let own = snapshot.by_pid(std::process::id()).unwrap().name.clone();
        let detector = ProcessDetector::new(&[own.clone()]);
        let signal = detector.detect(&snapshot).expect("own process detected");
        assert_eq!(signal.process_name.as_deref(), Some(own.as_str()));
    }
}
