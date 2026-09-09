//! Windows' own idea of what a game is.
//!
//! Windows keeps a Known Game List and expands it, per user, under
//! `HKCU\System\GameConfigStore\Children`. Each subkey describes one title.
//! The two fields that identify an executable are:
//!
//! - `MatchedExeFullPath`: the full path of the game executable.
//! - `ExeParentDirectory`: the directory the executable lives in. Inconsistent
//!   in practice: sometimes a full path, sometimes a bare folder name.
//!
//! Reading this is inert and needs no privileges, but the layout is
//! undocumented, so `status` prints what was parsed rather than asking anyone
//! to take it on faith.

use std::collections::BTreeSet;

use anyhow::Result;

use crate::registry::Key;

const CHILDREN_KEY: &str = r"System\GameConfigStore\Children";

/// Bare directory names too generic to identify a game on their own. Real
/// entries observed in the wild include `x64`, which would otherwise match any
/// executable sitting in a folder of that name.
const GENERIC_DIR_NAMES: &[&str] = &[
    "x64", "x86", "bin", "binaries", "win32", "win64", "game", "games", "release", "debug",
    "retail", "app", "data",
];

/// Why an executable was considered a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// The exact path is listed as `MatchedExeFullPath`.
    ExePath,
    /// The containing directory is listed as `ExeParentDirectory`, as a path.
    ParentPath,
    /// The containing directory's name is listed as `ExeParentDirectory`.
    ParentName,
}

impl MatchKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ExePath => "exe path",
            Self::ParentPath => "parent path",
            Self::ParentName => "parent name",
        }
    }
}

/// The list as loaded from the registry, normalised for lookups.
#[derive(Debug, Default, Clone)]
pub struct KnownGames {
    /// Lowercased full executable paths.
    exe_paths: BTreeSet<String>,
    /// Lowercased full directory paths.
    parent_paths: BTreeSet<String>,
    /// Lowercased bare directory names, generic ones already filtered out.
    parent_names: BTreeSet<String>,
    /// Entries seen, including those carrying no usable identity.
    pub entries: usize,
    /// Bare names dropped for being too generic, kept for `status`.
    pub skipped_generic: Vec<String>,
}

impl KnownGames {
    pub fn load() -> Result<Self> {
        let root = Key::open_current_user(CHILDREN_KEY)?;
        let mut list = Self::default();

        for name in root.subkey_names() {
            let Ok(child) = root.open_subkey(&name) else {
                continue;
            };
            list.entries += 1;

            if let Some(path) = child.string_value("MatchedExeFullPath") {
                list.exe_paths.insert(normalize(&path));
            }
            if let Some(parent) = child.string_value("ExeParentDirectory") {
                list.add_parent(&parent);
            }
        }
        Ok(list)
    }

    /// `ExeParentDirectory` holds either a full path or a bare folder name.
    fn add_parent(&mut self, parent: &str) {
        let value = normalize(parent);
        if value.contains('\\') || value.contains('/') {
            self.parent_paths.insert(value);
        } else if GENERIC_DIR_NAMES.contains(&value.as_str()) {
            self.skipped_generic.push(parent.to_owned());
        } else {
            self.parent_names.insert(value);
        }
    }

    /// Does Windows know this executable as a game?
    pub fn match_exe(&self, exe_path: &str) -> Option<MatchKind> {
        let exe = normalize(exe_path);
        if self.exe_paths.contains(&exe) {
            return Some(MatchKind::ExePath);
        }

        let path = std::path::Path::new(&exe);
        let parent = path.parent()?;
        if self
            .parent_paths
            .contains(&normalize(&parent.to_string_lossy()))
        {
            return Some(MatchKind::ParentPath);
        }
        let parent_name = normalize(&parent.file_name()?.to_string_lossy());
        if self.parent_names.contains(&parent_name) {
            return Some(MatchKind::ParentName);
        }
        None
    }

    pub fn counts(&self) -> Counts {
        Counts {
            entries: self.entries,
            exe_paths: self.exe_paths.len(),
            parent_paths: self.parent_paths.len(),
            parent_names: self.parent_names.len(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Counts {
    pub entries: usize,
    pub exe_paths: usize,
    pub parent_paths: usize,
    pub parent_names: usize,
}

/// Paths compare case-insensitively on Windows, and trailing separators are
/// noise.
fn normalize(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> KnownGames {
        let mut list = KnownGames::default();
        list.exe_paths
            .insert(normalize(r"D:\Games\Red Dead Redemption 2\RDR2.exe"));
        list.add_parent(r"C:\Games\Steam\steamapps\common\Wreckfest 2");
        list.add_parent("Assetto Corsa");
        list.add_parent("x64");
        list
    }

    #[test]
    fn exact_executable_paths_match_case_insensitively() {
        assert_eq!(
            list().match_exe(r"d:\games\red dead redemption 2\rdr2.exe"),
            Some(MatchKind::ExePath)
        );
    }

    #[test]
    fn a_parent_directory_given_as_a_path_matches() {
        assert_eq!(
            list().match_exe(r"C:\Games\Steam\steamapps\common\Wreckfest 2\Wreckfest2.exe"),
            Some(MatchKind::ParentPath)
        );
    }

    #[test]
    fn a_parent_directory_given_as_a_bare_name_matches() {
        assert_eq!(
            list().match_exe(r"E:\Whatever\Assetto Corsa\acs.exe"),
            Some(MatchKind::ParentName)
        );
    }

    #[test]
    fn generic_directory_names_are_ignored() {
        let list = list();
        assert_eq!(list.skipped_generic, vec!["x64".to_owned()]);
        assert_eq!(list.match_exe(r"C:\Tools\x64\notagame.exe"), None);
    }

    #[test]
    fn unrelated_executables_do_not_match() {
        assert_eq!(list().match_exe(r"C:\Windows\System32\notepad.exe"), None);
    }

    #[test]
    fn the_real_list_loads() {
        let list = KnownGames::load().expect("GameConfigStore is readable");
        assert!(list.entries > 0, "expected at least one known game entry");
    }
}
