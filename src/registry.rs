//! Minimal read-only registry access.
//!
//! A thin shape over Microsoft's `windows-registry`, the safe wrapper the
//! `windows` project ships for exactly this. What stays here is what the rest
//! of the program wants: open under a root with a failure that names the key,
//! walk subkeys, and read a string or a DWORD as an `Option` -- absent and
//! wrong-typed both mean "no", because every caller treats them the same.

use anyhow::{Context, Result};
use windows_registry::{CURRENT_USER, LOCAL_MACHINE};

/// An open registry key, closed on drop.
pub struct Key(windows_registry::Key);

impl Key {
    /// Open a key under `HKEY_CURRENT_USER` for reading.
    pub fn open_current_user(path: &str) -> Result<Self> {
        CURRENT_USER
            .open(path)
            .map(Self)
            .with_context(|| format!("cannot open HKCU\\{path}"))
    }

    /// Open a key under `HKEY_LOCAL_MACHINE` for reading.
    pub fn open_local_machine(path: &str) -> Result<Self> {
        LOCAL_MACHINE
            .open(path)
            .map(Self)
            .with_context(|| format!("cannot open HKLM\\{path}"))
    }

    pub fn open_subkey(&self, name: &str) -> Result<Self> {
        self.0
            .open(name)
            .map(Self)
            .with_context(|| format!("cannot open subkey `{name}`"))
    }

    /// Names of the immediate subkeys.
    pub fn subkey_names(&self) -> Vec<String> {
        self.0
            .keys()
            .map(|names| names.collect())
            .unwrap_or_default()
    }

    /// A `REG_SZ` value, or `None` when it is absent, not a string, or empty.
    pub fn string_value(&self, name: &str) -> Option<String> {
        self.0
            .get_string(name)
            .ok()
            .filter(|value| !value.is_empty())
    }

    /// A `REG_DWORD` value, or `None` when it is absent or not one.
    ///
    /// Windows keeps several of its own switches this way -- the taskbar theme
    /// among them -- so reading one is not the same job as reading a string.
    pub fn dword_value(&self, name: &str) -> Option<u32> {
        self.0.get_u32(name).ok()
    }
}
