//! Windows' own idea of what a game is.
//!
//! Windows keeps a Known Game List and expands it, per user, under
//! `HKCU\System\GameConfigStore\Children`. Each subkey describes one title.
//! Three fields identify one:
//!
//! - `MatchedExeFullPath`: the full path of the game executable.
//! - `ExeParentDirectory`: the directory the executable lives in. Inconsistent
//!   in practice: sometimes a full path, sometimes a bare folder name.
//! - `UtmItemId`: for packaged Store and Game Pass titles, which have no
//!   executable path at all. Shaped `P~<PackageFamilyName>!<AppId>`.
//!
//! `Type` tells the two families apart: 1 for Win32 titles, 2 for packaged
//! ones. Starfield is a `Type = 2` entry carrying only
//! `UtmItemId = P~BethesdaSoftworks.ProjectGold_3275kfvn8vcwc!Game`, so path
//! matching alone would never name it.
//!
//! Reading this is inert and needs no privileges, but the layout is
//! undocumented, so `status` prints what was parsed rather than asking anyone
//! to take it on faith. Detection does not depend on any of it: this only puts
//! a name on the game the presence writer already found.

use std::collections::BTreeSet;

use anyhow::Result;

use crate::registry::Key;

use super::GameSignal;
use super::process::{Snapshot, identity};

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
    /// A packaged title whose package family name is listed in `UtmItemId`.
    Package,
}

impl MatchKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ExePath => "exe path",
            Self::ParentPath => "parent path",
            Self::ParentName => "parent name",
            Self::Package => "package family",
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
    /// Lowercased package family names of packaged titles.
    packages: BTreeSet<String>,
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
            if let Some(utm) = child.string_value("UtmItemId")
                && let Some(family) = package_family_from_utm(&utm)
            {
                list.packages.insert(family);
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

    /// Is this package family name one of the known titles?
    pub fn match_package(&self, family: &str) -> bool {
        self.packages.contains(&normalize(family))
    }

    /// Find the running process Windows would call a game, so its name can
    /// feed the logs and the action placeholders. Detection never depends on
    /// this succeeding.
    pub fn identify(&self, snapshot: &Snapshot) -> Option<GameSignal> {
        self.candidates(snapshot).into_iter().next()
    }

    /// Every process the list matches, not just the first.
    ///
    /// A title usually brings several: a launcher stub, an anti-cheat service
    /// and the game all share an install folder or a package family, so they
    /// all match. Which one is the game is a separate question, and not one
    /// this list can answer.
    pub fn candidates(&self, snapshot: &Snapshot) -> Vec<GameSignal> {
        let mut found = Vec::new();
        for process in &snapshot.processes {
            let identity = identity(process.pid);

            if let Some(path) = &identity.path
                && let Some(kind) = self.match_exe(path)
            {
                found.push(signal(process.pid, &process.name, identity.path, kind));
                continue;
            }
            // The package family name is asked for unconditionally. An earlier
            // version only asked when the image path sat under WindowsApps,
            // which silently missed every Store game installed elsewhere: the
            // WindowsApps entry is a junction, and Windows resolves it, so a
            // Game Pass title installed in, say, C:\Games reports that path.
            if let Some(family) = &identity.package_family
                && self.match_package(family)
            {
                found.push(signal(
                    process.pid,
                    &process.name,
                    identity.path,
                    MatchKind::Package,
                ));
            }
        }
        found
    }

    pub fn counts(&self) -> Counts {
        Counts {
            entries: self.entries,
            exe_paths: self.exe_paths.len(),
            parent_paths: self.parent_paths.len(),
            parent_names: self.parent_names.len(),
            packages: self.packages.len(),
        }
    }
}

fn signal(pid: u32, name: &str, path: Option<String>, kind: MatchKind) -> GameSignal {
    GameSignal {
        source: kind.label(),
        process_name: Some(name.to_owned()),
        process_id: Some(pid),
        process_path: path,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Counts {
    pub entries: usize,
    pub exe_paths: usize,
    pub parent_paths: usize,
    pub parent_names: usize,
    pub packages: usize,
}

/// `UtmItemId` is shaped `P~<PackageFamilyName>!<AppId>`. A few entries hold a
/// bare GUID instead, which identifies nothing usable.
fn package_family_from_utm(value: &str) -> Option<String> {
    let value = value.trim();
    if value.starts_with('{') {
        return None;
    }
    // Drop a short kind prefix such as `P~`.
    let rest = match value.split_once('~') {
        Some((prefix, rest)) if prefix.len() <= 2 => rest,
        _ => value,
    };
    let family = rest
        .split_once('!')
        .map(|(family, _)| family)
        .unwrap_or(rest);
    (!family.is_empty()).then(|| normalize(family))
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
        list.packages
            .insert("bethesdasoftworks.projectgold_3275kfvn8vcwc".to_owned());
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
    fn package_family_names_are_extracted_from_utm_item_ids() {
        assert_eq!(
            package_family_from_utm("P~BethesdaSoftworks.ProjectGold_3275kfvn8vcwc!Game"),
            Some("bethesdasoftworks.projectgold_3275kfvn8vcwc".to_owned())
        );
        assert_eq!(
            package_family_from_utm("P~Microsoft.SeaofThieves_8wekyb3d8bbwe!AppAthenaShipping"),
            Some("microsoft.seaofthieves_8wekyb3d8bbwe".to_owned())
        );
        // Bare GUID entries identify nothing.
        assert_eq!(
            package_family_from_utm("{A364829E-02FE-4F2B-82F9-F5DD09458120}"),
            None
        );
    }

    #[test]
    fn packaged_titles_match_by_family_name() {
        let list = list();
        assert!(list.match_package("BethesdaSoftworks.ProjectGold_3275kfvn8vcwc"));
        assert!(!list.match_package("Microsoft.WindowsCalculator_8wekyb3d8bbwe"));
    }

    #[test]
    fn a_packaged_title_matches_wherever_it_is_installed() {
        // The Store lets a game be installed anywhere; the WindowsApps entry is
        // then a junction and Windows reports the resolved path. Matching a
        // packaged title must therefore not look at the path at all.
        let list = list();
        assert!(list.match_package("BethesdaSoftworks.ProjectGold_3275kfvn8vcwc"));
        assert_eq!(
            list.match_exe(r"C:\Games\Starfield\Content\Starfield.exe"),
            None,
            "the known game list has no executable path for a packaged title"
        );
    }

    // Reads this machine's registry. A GitHub-hosted Windows runner is a
    // server image without Game Bar, so the key is absent there; the build
    // script runs the ignored tests on a developer machine.
    #[test]
    #[ignore = "reads the Known Game List, which Windows Server runners do not have"]
    fn the_real_list_loads() {
        let list = KnownGames::load().expect("GameConfigStore is readable");
        assert!(list.entries > 0, "expected at least one known game entry");
        assert!(
            list.counts().packages > 0,
            "expected at least one packaged title"
        );
    }
}
