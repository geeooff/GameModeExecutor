//! Process enumeration and identity.

use anyhow::{Context, Result};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::K32EnumProcesses;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::PWSTR;

/// A borrowed Win32 handle closed on drop.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: every `OwnedHandle` wraps a handle this module opened, and
        // this is the one place it is closed.
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
        // SAFETY: no pointers go in; the handle that comes out is owned below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
            .context("CreateToolhelp32Snapshot failed")?;
        let snapshot = OwnedHandle(snapshot);

        let mut processes = Vec::with_capacity(256);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        // SAFETY: `entry.dwSize` is set to the struct's size, which is the
        // contract for these two calls, and the snapshot handle is open.
        let mut ok = unsafe { Process32FirstW(snapshot.0, &mut entry) };
        while ok.is_ok() {
            processes.push(ProcessInfo {
                pid: entry.th32ProcessID,
                name: from_wide_nul(&entry.szExeFile),
            });
            // SAFETY: as for `Process32FirstW`.
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
    // SAFETY: no memory preconditions; a refused or vanished process makes
    // the call fail, which is the `None` case.
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .ok()
        .map(OwnedHandle)
}

/// What a process can tell us about itself. Both fields are best effort: a
/// process we may not open yields neither.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    /// Full image path, with junctions already resolved by Windows.
    pub path: Option<String>,
    /// Package family name for a packaged (Store or Game Pass) process, e.g.
    /// `BethesdaSoftworks.ProjectGold_3275kfvn8vcwc`. `None` for ordinary Win32
    /// processes, which is the common case.
    pub package_family: Option<String>,
}

/// Ask a process both questions on one handle.
///
/// Doing this in one pass matters: a packaged game may run from anywhere the
/// user chose to install it, so the package family name has to be asked for
/// unconditionally rather than only for processes that look packaged from
/// their path.
pub fn identity(pid: u32) -> Identity {
    let Some(handle) = open_for_query(pid) else {
        return Identity::default();
    };
    Identity {
        path: image_path(&handle),
        package_family: family_name(&handle),
    }
}

/// Best-effort full image path.
pub fn full_path(pid: u32) -> Option<String> {
    identity(pid).path
}

/// Every process id, and nothing else: `K32EnumProcesses`, about 32 us on
/// the machine Lot 15 was measured on against the snapshot's 4 ms.
pub fn ids() -> Result<Vec<u32>> {
    let mut ids = vec![0u32; 1024];
    loop {
        let mut needed = 0u32;
        let size = (ids.len() * size_of::<u32>()) as u32;
        // SAFETY: the buffer and its size in bytes are passed together, and
        // `needed` is a local out pointer.
        unsafe { K32EnumProcesses(ids.as_mut_ptr(), size, &mut needed) }
            .ok()
            .context("K32EnumProcesses failed")?;
        // The documented sign of a buffer too small is a full one.
        if needed < size {
            ids.truncate(needed as usize / size_of::<u32>());
            return Ok(ids);
        }
        ids.resize(ids.len() * 2, 0);
    }
}

/// Which processes run, each known by its lowercased file name, kept up to
/// date for little: the ids alone every look, a name asked only of a
/// process not seen before, and a full snapshot every [`Tracker::FULL_EVERY`].
///
/// The snapshot is the net for a process id reused between two looks: an id
/// identifies a process only while it lives (Microsoft's documentation), and
/// a freed one came back 2.8 s later at the soonest under fifty new
/// processes a second (measured 2026-09-23). A game that took such an id
/// looks already seen until the next snapshot names it afresh.
#[derive(Debug, Default)]
pub struct Tracker {
    /// `None` for a process that would not say its name.
    names: std::collections::HashMap<u32, Option<String>>,
    snapshot_at: Option<std::time::Instant>,
}

impl Tracker {
    pub const FULL_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

    /// Bring the names up to date.
    pub fn refresh(&mut self, now: std::time::Instant) -> Result<()> {
        let due = self
            .snapshot_at
            .is_none_or(|at| now.saturating_duration_since(at) >= Self::FULL_EVERY);
        if due {
            let snapshot = Snapshot::take()?;
            self.names = snapshot
                .processes
                .into_iter()
                .map(|process| (process.pid, Some(process.name.to_lowercase())))
                .collect();
            self.snapshot_at = Some(now);
            return Ok(());
        }
        let ids = ids()?;
        self.update(&ids, |pid| {
            full_path(pid).map(|path| {
                path.rsplit(['\\', '/'])
                    .next()
                    .unwrap_or(&path)
                    .to_lowercase()
            })
        });
        Ok(())
    }

    /// The ids now running: the gone are forgotten, the new are named.
    fn update(&mut self, ids: &[u32], name_of: impl Fn(u32) -> Option<String>) {
        let running: std::collections::HashSet<u32> = ids.iter().copied().collect();
        self.names.retain(|pid, _| running.contains(pid));
        for &pid in ids {
            self.names.entry(pid).or_insert_with(|| name_of(pid));
        }
    }

    /// The processes of this lowercased file name.
    pub fn named<'a>(&'a self, lower_name: &'a str) -> impl Iterator<Item = u32> + 'a {
        self.names
            .iter()
            .filter(move |(_, name)| name.as_deref() == Some(lower_name))
            .map(|(&pid, _)| pid)
    }
}

/// Process owning the foreground window.
pub fn foreground_pid() -> Option<u32> {
    // SAFETY: no arguments and no preconditions; a null handle is checked.
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer. A window that vanished since the
    // call above makes the API return 0, which leaves `pid` untouched.
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

fn image_path(handle: &OwnedHandle) -> Option<String> {
    let mut buffer = [0u16; 32768];
    let mut size = buffer.len() as u32;
    // SAFETY: `size` tells the API how many UTF-16 units the buffer holds, so
    // the write is bounded; the handle is open for as long as `handle` lives.
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

fn family_name(handle: &OwnedHandle) -> Option<String> {
    let mut length = 0u32;
    // First call sizes the buffer; it fails with ERROR_INSUFFICIENT_BUFFER for
    // a packaged process and with APPMODEL_ERROR_NO_PACKAGE otherwise.
    // SAFETY: with no buffer the API only writes the required length.
    unsafe { GetPackageFamilyName(handle.0, &mut length, None) }
        .ok()
        .err()?;
    if length == 0 {
        return None;
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: the buffer holds exactly the `length` the API asked for.
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
    fn the_ids_include_this_process() {
        assert!(ids().unwrap().contains(&std::process::id()));
    }

    /// Only the processes not seen before are asked their name; the gone are
    /// forgotten, so a later process under a freed id is asked afresh.
    #[test]
    fn the_tracker_names_only_what_is_new() {
        let asked = std::cell::RefCell::new(Vec::new());
        let name_of = |pid: u32| {
            asked.borrow_mut().push(pid);
            (pid != 666).then(|| format!("p{pid}.exe"))
        };
        let mut tracker = Tracker::default();
        tracker.update(&[4, 8, 666], name_of);
        assert_eq!(*asked.borrow(), vec![4, 8, 666]);
        assert_eq!(tracker.named("p8.exe").collect::<Vec<_>>(), vec![8]);
        assert_eq!(
            tracker.named("p666.exe").count(),
            0,
            "unnamed stays unnamed"
        );

        asked.borrow_mut().clear();
        tracker.update(&[4, 12], name_of);
        assert_eq!(*asked.borrow(), vec![12], "4 was known, 8 went, 12 is new");
        assert_eq!(tracker.named("p8.exe").count(), 0);

        asked.borrow_mut().clear();
        tracker.update(&[4, 8], name_of);
        assert_eq!(*asked.borrow(), vec![8], "a returning id is a new process");
    }

    #[test]
    fn the_tracker_starts_from_a_full_snapshot() {
        let mut tracker = Tracker::default();
        tracker.refresh(std::time::Instant::now()).unwrap();
        let own = identity(std::process::id())
            .path
            .map(|path| path.rsplit('\\').next().unwrap().to_lowercase())
            .unwrap();
        assert!(tracker.named(&own).any(|pid| pid == std::process::id()));
    }

    #[test]
    fn an_unpackaged_process_has_no_package_family_name() {
        // The test binary is a plain Win32 executable.
        assert_eq!(identity(std::process::id()).package_family, None);
    }
}
