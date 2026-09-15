//! Minimal read-only registry access.

use anyhow::{Context, Result};
use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegEnumKeyExW,
    RegOpenKeyExW, RegQueryValueExW,
};
use windows::core::{PCWSTR, PWSTR};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// An open registry key, closed on drop.
pub struct Key(HKEY);

impl Key {
    /// Open a key under `HKEY_CURRENT_USER` for reading.
    pub fn open_current_user(path: &str) -> Result<Self> {
        Self::open(HKEY_CURRENT_USER, "HKCU", path)
    }

    /// Open a key under `HKEY_LOCAL_MACHINE` for reading. Reading needs no
    /// elevation, unlike writing.
    pub fn open_local_machine(path: &str) -> Result<Self> {
        Self::open(HKEY_LOCAL_MACHINE, "HKLM", path)
    }

    fn open(root: HKEY, root_name: &str, path: &str) -> Result<Self> {
        let subkey = wide(path);
        let mut key = HKEY::default();
        unsafe {
            RegOpenKeyExW(root, PCWSTR(subkey.as_ptr()), None, KEY_READ, &mut key)
                .ok()
                .with_context(|| format!("cannot open {root_name}\\{path}"))?;
        }
        Ok(Self(key))
    }

    pub fn open_subkey(&self, name: &str) -> Result<Self> {
        let subkey = wide(name);
        let mut key = HKEY::default();
        unsafe {
            RegOpenKeyExW(self.0, PCWSTR(subkey.as_ptr()), None, KEY_READ, &mut key)
                .ok()
                .with_context(|| format!("cannot open subkey `{name}`"))?;
        }
        Ok(Self(key))
    }

    /// Names of the immediate subkeys.
    pub fn subkey_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        // Key names are capped at 255 characters plus the terminator.
        let mut buffer = [0u16; 256];
        for index in 0.. {
            let mut length = buffer.len() as u32;
            let result = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &mut length,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if result == ERROR_NO_MORE_ITEMS {
                break;
            }
            if result.is_err() {
                break;
            }
            names.push(String::from_utf16_lossy(&buffer[..length as usize]));
        }
        names
    }

    /// A `REG_SZ` value, or `None` when it is absent or not a string.
    pub fn string_value(&self, name: &str) -> Option<String> {
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
            let value = String::from_utf16_lossy(&units);
            (!value.is_empty()).then_some(value)
        }
    }

    /// A `REG_DWORD` value, or `None` when it is absent or the wrong size.
    ///
    /// Windows keeps several of its own switches this way -- the taskbar theme
    /// among them -- so reading one is not the same job as reading a string.
    pub fn dword_value(&self, name: &str) -> Option<u32> {
        let name = wide(name);
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                None,
                Some(std::ptr::from_mut(&mut value).cast()),
                Some(&mut size),
            )
            .ok()
            .ok()?;
        }
        (size as usize == std::mem::size_of::<u32>()).then_some(value)
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { _ = RegCloseKey(self.0) };
    }
}
