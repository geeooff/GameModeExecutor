//! The games the person marked by hand.
//!
//! Ticking *Remember this is a game* in the Game Bar writes an entry into
//! Windows' game list, `HKCU\System\GameConfigStore\Children`, with
//! `Revision = 1` and no Xbox `TitleId`. Windows treats the title as a game
//! from then on -- the overlay, capture, Game Mode -- but never starts the
//! presence writer for it: the writer tells Xbox what is being played, and
//! a title named by hand has no Xbox identity to tell. Measured on
//! 2026-09-20 and 2026-09-23; `docs/design/15-marked-games.md` has the runs.
//!
//! So these entries are the second signal: a running process whose full
//! path is one of theirs is a game session, by Windows' own list. Matched on
//! the exact path only -- the parent-directory and package rules naming uses
//! match too loosely to decide that a session exists.
//!
//! Change notifications on the list were measured and never arrive for the
//! Game Bar's writes, so the list is read again only when its key's
//! last-write time moves, which a tick or an untick does: one query a poll.

use anyhow::Result;

use crate::registry::Key;

/// Windows' game list, per user.
pub const LIST_KEY: &str = r"System\GameConfigStore\Children";

/// The revision every entry ticked by hand carries. Entries Windows creates
/// from Microsoft's list carry that list's revision -- 2691 on the machine
/// the lot was measured on -- or 2 for packaged titles.
const HAND_MADE_REVISION: u32 = 1;

/// One entry the person made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The executable's full path, as Windows wrote it.
    pub path: String,
    /// The same, lowercased, for comparing with a process's path.
    lower_path: String,
    /// The executable's file name, lowercased, for comparing with a
    /// process's name before its path is asked for.
    file_name: String,
}

impl Entry {
    pub fn new(path: &str) -> Self {
        let lower_path = path.to_lowercase();
        let file_name = file_name_of(&lower_path).to_owned();
        Self {
            path: path.to_owned(),
            lower_path,
            file_name,
        }
    }

    /// The executable's file name as Windows wrote it, for the log.
    pub fn display_name(&self) -> &str {
        file_name_of(&self.path)
    }

    /// The executable's file name, lowercased, to find its processes among
    /// the running ones before their paths are asked for.
    pub fn file_name_lower(&self) -> &str {
        &self.file_name
    }

    /// Whether a process at this full path is this entry's.
    pub fn is(&self, path: &str) -> bool {
        path.to_lowercase() == self.lower_path
    }
}

/// The file name of a path, whichever separator it uses.
pub fn file_name_of(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// The rule, apart from the registry: an entry with an executable path, the
/// hand-made revision, and no Xbox title id.
pub fn is_hand_made(revision: Option<u32>, has_title_id: bool, exe: Option<&str>) -> bool {
    revision == Some(HAND_MADE_REVISION) && !has_title_id && exe.is_some_and(|exe| !exe.is_empty())
}

/// The hand-made entries in the list now.
pub fn load() -> Result<Vec<Entry>> {
    let root = Key::open_current_user(LIST_KEY)?;
    let mut entries = Vec::new();
    for name in root.subkey_names() {
        let Ok(child) = root.open_subkey(&name) else {
            continue;
        };
        let exe = child.string_value("MatchedExeFullPath");
        // Seen as a string on every entry read so far; a number is accepted
        // too, since only its presence matters.
        let has_title_id =
            child.string_value("TitleId").is_some() || child.dword_value("TitleId").is_some();
        if is_hand_made(child.dword_value("Revision"), has_title_id, exe.as_deref())
            && let Some(exe) = exe
        {
            entries.push(Entry::new(&exe));
        }
    }
    Ok(entries)
}

/// Which of `entries` Microsoft's own list covers, read from the file
/// Windows keeps it in. `Err` when that file cannot be read.
pub fn covered_by_microsoft(entries: &[Entry]) -> std::io::Result<Vec<&Entry>> {
    let path = super::microsoft_list::path()
        .ok_or_else(|| std::io::Error::other("no local profile to find it in"))?;
    let list = super::microsoft_list::List::new(super::microsoft_list::read(&path)?);
    Ok(covered(entries, &list))
}

/// Which of `entries` `list` covers: the rule, apart from the file.
pub fn covered<'a>(entries: &'a [Entry], list: &super::microsoft_list::List) -> Vec<&'a Entry> {
    entries
        .iter()
        .filter(|entry| list.covers(&entry.path))
        .collect()
}

/// Say, once, which games marked by hand Microsoft's list now knows, so the
/// person can untick them and let Windows recognise them by itself -- asked
/// for by the maintainer on 2026-09-23, to clean up boxes ticked before
/// Microsoft listed the game. Called at the watcher's start and never again
/// until the next: the list changes with Windows' own updates, not by the
/// minute.
pub fn say_what_microsoft_now_covers() {
    let entries = match load() {
        Ok(entries) => entries,
        Err(error) => {
            tracing::debug!(
                target: crate::logging::target::GAME,
                error = %format!("{error:#}"),
                "The games marked by hand cannot be read, so nothing is said about them"
            );
            return;
        }
    };
    if entries.is_empty() {
        return;
    }
    let covered = match covered_by_microsoft(&entries) {
        Ok(covered) => covered,
        Err(error) => {
            tracing::debug!(
                target: crate::logging::target::GAME,
                error = %error,
                "Microsoft's game list cannot be read, so nothing is said about the games \
                 marked by hand"
            );
            return;
        }
    };
    tracing::debug!(
        target: crate::logging::target::GAME,
        marked = entries.len(),
        covered = covered.len(),
        "Games marked by hand, and how many Microsoft's list now covers"
    );
    for entry in covered {
        tracing::info!(
            target: crate::logging::target::GAME,
            path = %entry.path,
            "{} is marked as a game by hand, and Microsoft's own list knows it now: untick \
             \"Remember this is a game\" in the Game Bar, and Windows will recognise it by itself",
            entry.display_name()
        );
    }
}

/// The list's key, kept open to ask when it was last written.
pub struct Watch {
    key: Option<Key>,
    stamp: Option<u64>,
}

impl Watch {
    pub fn new() -> Self {
        Self {
            key: Key::open_current_user(LIST_KEY).ok(),
            stamp: None,
        }
    }

    /// Whether the list may have gained or lost an entry since the last
    /// call. The first call always says yes. A key that could not be opened
    /// is tried again -- a profile where the Game Bar has written nothing yet
    /// has no list until the first game -- and until it opens, and whenever
    /// it cannot be asked, the answer is no and the entries read before stay.
    pub fn changed(&mut self) -> bool {
        if self.key.is_none() {
            self.key = Key::open_current_user(LIST_KEY).ok();
        }
        let Some(stamp) = self.key.as_ref().and_then(Key::last_write) else {
            return false;
        };
        let changed = self.stamp != Some(stamp);
        self.stamp = Some(stamp);
        changed
    }
}

impl Default for Watch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the maintainer's machine showed on 2026-09-23: the entries ticked
    /// by hand carry revision 1 and no title id; the ones Windows made from
    /// Microsoft's list carry its revision and a title id; packaged titles
    /// carry revision 2 and a package id instead of a path.
    #[test]
    fn only_entries_ticked_by_hand_count() {
        let exe = Some(r"C:\Games\The Other Side\TheOtherSide-Win64-Shipping.exe");
        assert!(is_hand_made(Some(1), false, exe));
        assert!(!is_hand_made(Some(2691), true, exe), "listed by Microsoft");
        assert!(!is_hand_made(Some(1), true, exe), "a title id is Xbox's");
        assert!(!is_hand_made(Some(2), false, None), "a packaged title");
        assert!(
            !is_hand_made(Some(1), false, Some("")),
            "no path, nothing to match"
        );
        assert!(!is_hand_made(None, false, exe));
    }

    #[test]
    fn a_process_is_matched_by_its_whole_path_whatever_the_case() {
        let entry = Entry::new(r"D:\Games\Steam\steamapps\common\The Other Side\TOS.exe");
        assert_eq!(entry.display_name(), "TOS.exe");
        assert_eq!(entry.file_name_lower(), "tos.exe");
        assert!(entry.is(r"d:\games\steam\steamapps\common\the other side\tos.exe"));
        assert!(
            !entry.is(r"C:\Elsewhere\TOS.exe"),
            "same name, another game"
        );
    }

    /// The two titles the maintainer had ticked before Microsoft listed them,
    /// and one it has never listed: only the first two are said.
    #[test]
    fn only_the_entries_microsofts_list_covers_are_said() {
        use crate::detect::microsoft_list::{List, fixture::record};
        let mut bytes = record("DS2.exe", &["common", "DEATH STRANDING 2 - ON THE BEACH"]);
        bytes.extend(record("Wreckfest2.exe", &["common", "Wreckfest 2"]));
        let list = List::new(bytes);
        let entries = [
            Entry::new(r"C:\Games\Steam\steamapps\common\DEATH STRANDING 2 - ON THE BEACH\DS2.exe"),
            Entry::new(r"D:\Games\Steam\steamapps\common\The Other Side\TOS.exe"),
            Entry::new(r"C:\Games\Steam\steamapps\common\Wreckfest 2\Wreckfest2.exe"),
        ];
        let said: Vec<&str> = covered(&entries, &list)
            .into_iter()
            .map(Entry::display_name)
            .collect();
        assert_eq!(said, ["DS2.exe", "Wreckfest2.exe"]);
    }

    #[test]
    #[ignore = "reads Windows' game list, absent on Windows Server runners"]
    fn the_real_list_and_its_stamp_are_readable() {
        load().expect("the game list is readable");
        let mut watch = Watch::new();
        assert!(watch.changed(), "the first look always says changed");
        assert!(!watch.changed(), "and then not, with nothing written");
    }
}
