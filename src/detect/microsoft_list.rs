//! Microsoft's own list of games, read for one question only: does it know a
//! title the person marked by hand?
//!
//! Windows keeps the Known Game List Microsoft distributes as a file,
//! `%LOCALAPPDATA%\Microsoft\GameDVR\KnownGameList.bin`, and creates the
//! registry entries of `HKCU\System\GameConfigStore\Children` from it. A
//! title ticked *Remember this is a game* before Microsoft listed it keeps
//! Windows on the hand-made entry -- no Xbox identity, no presence writer --
//! until the box is unticked: measured on 2026-09-23 with DS2 and
//! Wreckfest 2. So at start the watcher says which hand-made entries this
//! file covers, and the person can untick them.
//!
//! **The format is not documented.** What is relied on was read from the
//! file on 2026-09-23 and is recorded in `docs/design/15-marked-games.md`:
//! the executable's name is a field of its own, UTF-16, preceded by its
//! length in bytes as 16 bits; the title's folder names follow as UTF-16
//! strings each preceded by their length as 32 bits, with other bytes
//! between them, and a GUID in the same shape ends them. Anything else in
//! the file is ignored. A file that is missing, or holds no record shaped
//! like that for a name, gives no hint: the hint is advice, and wrong advice
//! -- untick a game Windows would then not recognise -- is worse than none.

use std::path::{Path, PathBuf};

/// Where Windows keeps the list for this user.
pub fn path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|local| {
        PathBuf::from(local)
            .join("Microsoft")
            .join("GameDVR")
            .join("KnownGameList.bin")
    })
}

/// How far after the executable's name its folder names are looked for:
/// room for three folder names of the longest length taken. The records
/// read on 2026-09-23 had theirs within 200 bytes.
const RECORD_REACH: usize = 3 * (4 + LONGEST_NAME) + 64;

/// The longest folder name taken as one, in bytes: a path component.
const LONGEST_NAME: usize = 520;

/// The shortest, in bytes: two characters.
const SHORTEST_NAME: usize = 4;

/// The list, read once and decoded once: the bytes, for the fields that
/// follow a name, and the same bytes as UTF-16 at both alignments, for
/// finding the names -- nothing says a record starts on an even byte.
pub struct List {
    bytes: Vec<u8>,
    units: [Vec<u16>; 2],
}

impl List {
    pub fn new(bytes: Vec<u8>) -> Self {
        let decode = |start: usize| -> Vec<u16> {
            bytes[start.min(bytes.len())..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect()
        };
        let units = [decode(0), decode(1)];
        Self { bytes, units }
    }

    /// Whether the list has a record for the executable at `exe_path`: one
    /// whose executable field is that file name, whole, and each of whose
    /// folder names is a folder of that path -- the two things the record
    /// gives Windows to match on. A record with no folder names is not taken
    /// as a match: too little to be sure of.
    pub fn covers(&self, exe_path: &str) -> bool {
        let mut components: Vec<String> =
            exe_path.split(['\\', '/']).map(str::to_lowercase).collect();
        let Some(file_name) = exe_path.rsplit(['\\', '/']).next() else {
            return false;
        };
        components.pop();
        self.records(file_name).iter().any(|folders| {
            !folders.is_empty()
                && folders
                    .iter()
                    .all(|folder| components.contains(&folder.to_lowercase()))
        })
    }

    /// The folder names of every record whose executable field is
    /// `file_name`, compared without regard to case.
    fn records(&self, file_name: &str) -> Vec<Vec<String>> {
        let needle: Vec<u16> = file_name.encode_utf16().map(lower).collect();
        if needle.is_empty() {
            return Vec::new();
        }
        let length = (needle.len() * 2) as u16;
        let mut found = Vec::new();
        for (start, units) in self.units.iter().enumerate() {
            for at in 1..=units.len().saturating_sub(needle.len()) {
                if units[at - 1] == length
                    && units[at..at + needle.len()]
                        .iter()
                        .zip(&needle)
                        .all(|(unit, wanted)| lower(*unit) == *wanted)
                {
                    let after = start + (at + needle.len()) * 2;
                    found.push(folders_after(&self.bytes, after));
                }
            }
        }
        found
    }
}

/// One UTF-16 unit in lower case, when its lower case is one unit too --
/// `É` to `é` as well as `A` to `a` -- and as it is otherwise.
fn lower(unit: u16) -> u16 {
    let Some(character) = char::from_u32(u32::from(unit)) else {
        return unit;
    };
    let mut lowered = character.to_lowercase();
    match (lowered.next(), lowered.next()) {
        (Some(one), None) => u16::try_from(u32::from(one)).unwrap_or(unit),
        _ => unit,
    }
}

/// The strings that follow a record's executable name up to the GUID that
/// ends them, each preceded by its length in bytes as 32 bits. A folder
/// name is taken from two characters up: the bytes between two fields can
/// read as a one-character string -- `02 00 00 00` then the low bytes of the
/// next length, `>` before `Counter-Strike Global Offensive` in a first
/// reading -- and no folder of a record read so far was that short.
fn folders_after(list: &[u8], from: usize) -> Vec<String> {
    let end = (from + RECORD_REACH).min(list.len());
    let mut folders = Vec::new();
    let mut at = from;
    while at + 4 <= end {
        let size =
            u32::from_le_bytes([list[at], list[at + 1], list[at + 2], list[at + 3]]) as usize;
        let text_end = at + 4 + size;
        if (SHORTEST_NAME..=LONGEST_NAME).contains(&size)
            && size.is_multiple_of(2)
            && text_end <= list.len()
            && let Some(text) = printable(&list[at + 4..text_end])
        {
            if is_guid(&text) {
                return folders;
            }
            folders.push(text);
            at = text_end;
            continue;
        }
        at += 2;
    }
    // No GUID within reach: not a record of the shape this relies on.
    Vec::new()
}

/// UTF-16 made only of printable characters, or `None`.
fn printable(bytes: &[u8]) -> Option<String> {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let text = String::from_utf16(&units).ok()?;
    text.chars().all(|c| !c.is_control()).then_some(text)
}

fn is_guid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Read the file once. A file missing or unreadable is said at `debug` by
/// the caller, and gives no hint.
pub fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// Records shaped like the real file's, for the tests here and in
/// `hand_made`.
#[cfg(test)]
pub(crate) mod fixture {
    pub(crate) fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// A record shaped like the ones read on 2026-09-23: some bytes, the
    /// executable's name with its length as 16 bits, a few bytes, each
    /// folder name with its length as 32 bits, the GUID the same way, the
    /// title id after.
    pub(crate) fn record(exe: &str, folders: &[&str]) -> Vec<u8> {
        let mut bytes = vec![0xe0, 0x00, 0x00, 0x00];
        let name = utf16(exe);
        bytes.extend((name.len() as u16).to_le_bytes());
        bytes.extend(name);
        bytes.extend([0x02, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, 0x00]);
        for folder in folders {
            let text = utf16(folder);
            bytes.extend((text.len() as u32).to_le_bytes());
            bytes.extend(text);
            bytes.extend([0x01, 0x00, 0x00, 0x00]);
        }
        let guid = utf16("e26eb51c-9cba-4cc5-9e7e-bb1628b17f80");
        bytes.extend((guid.len() as u32).to_le_bytes());
        bytes.extend(guid);
        bytes.extend([0x02, 0x00, 0x00, 0x20]);
        let title = utf16("2076696971");
        bytes.extend((title.len() as u32).to_le_bytes());
        bytes.extend(title);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{record, utf16};
    use super::*;

    fn covers(list: &[u8], exe_path: &str) -> bool {
        List::new(list.to_vec()).covers(exe_path)
    }

    fn list() -> Vec<u8> {
        let mut list = vec![
            0xc4, 0x02, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x83, 0x0a, 0x00, 0x00,
        ];
        list.extend(record("borderlands2.exe", &["Borderlands 2", "Win32"]));
        list.extend(record("blazing chrome.exe", &["Blazing Chrome"]));
        list.extend(record(
            "cs2.exe",
            &["Counter-Strike Global Offensive", "win64"],
        ));
        list.extend(record("DS2.exe", &["DEATH STRANDING 2 - ON THE BEACH"]));
        list
    }

    #[test]
    fn a_listed_title_is_found_by_its_name_and_its_folders() {
        let list = list();
        assert!(covers(
            &list,
            r"D:\Games\Steam\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe"
        ));
        assert!(covers(
            &list,
            r"C:\Games\Steam\steamapps\common\DEATH STRANDING 2 - ON THE BEACH\DS2.exe"
        ));
    }

    /// Both were a searched-for name inside a longer field in the real file:
    /// `DS2.exe` ends `borderlands2.exe`, `chrome.exe` ends
    /// `blazing chrome.exe`.
    #[test]
    fn a_name_inside_another_field_is_not_a_record() {
        let list = list();
        assert!(!covers(
            &list,
            r"C:\Program Files\Google\Chrome\Application\chrome.exe"
        ));
        let mut only_borderlands = Vec::new();
        only_borderlands.extend(record("borderlands2.exe", &["Borderlands 2"]));
        assert!(!covers(
            &only_borderlands,
            r"C:\Games\Borderlands 2\DS2.exe"
        ));
    }

    #[test]
    fn the_same_name_in_another_folder_is_another_title() {
        let list = list();
        assert!(!covers(&list, r"C:\Games\Something Else\DS2.exe"));
        assert!(
            !covers(&list, r"D:\Counter-Strike Global Offensive\cs2.exe"),
            "every folder of the record must be there, win64 included"
        );
    }

    #[test]
    fn an_unlisted_title_and_an_unreadable_list_say_nothing() {
        assert!(!covers(
            &list(),
            r"D:\Games\The Other Side\TheOtherSide-Win64-Shipping.exe"
        ));
        assert!(!covers(&[], r"C:\Games\DS2.exe"));
        assert!(!covers(&[0xff; 64], r"C:\Games\DS2.exe"));
    }

    /// DS2's record, byte for byte as the real file has it: `common` then the
    /// title's folder, with `01 00 00 00` between them and nothing after the
    /// second. A first version wanted each name's length repeated after it,
    /// which Counter-Strike's record seemed to show, and missed this one.
    #[test]
    fn death_strandings_record_as_the_file_has_it() {
        let mut list = vec![0x33, 0x00, 0xe0, 0x00, 0x00, 0x00];
        let name = utf16("ds2.exe");
        list.extend((name.len() as u16).to_le_bytes());
        list.extend(name);
        list.extend([0x02, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, 0x00]);
        list.extend(12u32.to_le_bytes());
        list.extend(utf16("common"));
        list.extend([0x01, 0x00, 0x00, 0x00]);
        list.extend(64u32.to_le_bytes());
        list.extend(utf16("DEATH STRANDING 2 - ON THE BEACH"));
        list.extend([0x00, 0x00, 0x00, 0x20]);
        list.extend(72u32.to_le_bytes());
        list.extend(utf16("f5ec2e1c-0624-402c-8cf2-34c8fd856704"));
        assert!(covers(
            &list,
            r"C:\Games\Steam\steamapps\common\DEATH STRANDING 2 - ON THE BEACH\DS2.exe"
        ));
        assert!(
            !covers(&list, r"C:\Games\DEATH STRANDING 2 - ON THE BEACH\DS2.exe"),
            "`common` is one of the record's folders too"
        );
    }

    /// The names are compared without regard to case beyond ASCII: a title
    /// whose executable carries an accent is found whatever its case.
    #[test]
    fn an_accented_name_is_found_whatever_its_case() {
        let list = record("Élan.exe", &["Élan Vital"]);
        assert!(covers(&list, r"D:\Games\élan vital\élan.exe"));
        assert!(covers(&list, r"D:\Games\ÉLAN VITAL\ÉLAN.EXE"));
    }

    #[test]
    fn a_record_with_no_guid_within_reach_is_not_trusted() {
        let mut list = vec![0u8; 4];
        let name = utf16("DS2.exe");
        list.extend((name.len() as u16).to_le_bytes());
        list.extend(name);
        let folder = utf16("DEATH STRANDING 2 - ON THE BEACH");
        list.extend((folder.len() as u32).to_le_bytes());
        list.extend(folder);
        assert!(!covers(
            &list,
            r"C:\Games\Steam\steamapps\common\DEATH STRANDING 2 - ON THE BEACH\DS2.exe"
        ));
    }

    /// The real file, on a client machine. Microsoft's list has carried
    /// Counter-Strike for years and has no reason to carry a browser.
    #[test]
    #[ignore = "reads Microsoft's game list, absent on Windows Server runners"]
    fn the_real_list_knows_counter_strike_and_not_a_browser() {
        let list = read(&path().unwrap()).expect("the list is on this machine");
        assert!(covers(
            &list,
            r"D:\Games\Steam\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe"
        ));
        assert!(!covers(
            &list,
            r"C:\Program Files\Google\Chrome\Application\chrome.exe"
        ));
    }
}
