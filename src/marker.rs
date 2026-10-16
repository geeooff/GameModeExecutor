//! A file that says "a game session is open and its stop commands have not run".
//!
//! Written when the start commands run, removed once the stop commands ran
//! without a failure. Found at the next start, it means the last session never
//! closed -- a logoff, a shutdown, a crash, a power cut, a failed command --
//! and the stop commands run then instead.
//!
//! Its `state` says whether a watcher still held the session when it was last
//! written. `open` is written with the session; `left` once a watcher stops
//! in an orderly way and leaves the session to the next start -- an update, a
//! reload, a session end, a stop command that failed. A start that finds
//! `open` knows the last watcher ended without stopping: ended from outside,
//! or the machine stopped. Added 2026-10-16, when two watchers had vanished
//! mid-game, here and on a second machine, and their logs could not say so.
//!
//! This exists because of a measurement, not a hypothetical. A real logoff on
//! 2026-09-16 showed that a process started even one millisecond after
//! `WM_QUERYENDSESSION` dies with `STATUS_DLL_INIT_FAILED`: the session-end
//! handshake works, and the command it starts is stillborn. So the stop
//! commands cannot run at session end, and the only moment that does not
//! depend on Windows' timing is the next start.
//!
//! It lives at the root of `%LOCALAPPDATA%\GameModeExecutor`, not in `logs\`.
//! A logs folder is disposable by nature and gets emptied without a second
//! thought, which would take a pending recovery with it; state does not belong
//! among files anyone is entitled to throw away. Not next to the configuration
//! either: that may sit in `%APPDATA%`, which roams, and a marker following the
//! profile to another machine would run the stop commands there. Local,
//! per-user, non-roaming is exactly what the marker is about.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// No extension on purpose. `.txt` says "a note for a person" and is the first
/// thing a tidy-up deletes; a bare name says "the program's business". The
/// comment lines inside are for whoever opens it anyway.
pub const FILE_NAME: &str = "pending-stop-actions";

/// The second file at the same root: "the configuration could not be used
/// when the watcher last looked". Written when a fault is found, removed
/// when a usable file is read -- and *that* removal, at a start, is what
/// tells the watcher to say the fault is over rather than start in silence.
/// The maintainer broke the file, stopped, fixed it, started again, and got
/// no word on 2026-09-20; a fault outlives the process, so its memory must.
pub const FAULT_FILE_NAME: &str = "configuration-fault";

#[derive(Clone)]
pub struct Marker {
    path: PathBuf,
}

/// A watcher holds the session.
const OPEN: &str = "open";
/// A watcher stopped and left the session to the next start.
const LEFT: &str = "left";

/// What a marker left behind says about the session it belonged to. Every
/// field is informational; the file's presence is the signal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pending {
    pub game: Option<String>,
    pub since: Option<String>,
    /// The file still says `state = open`: the watcher that held the session
    /// ended without stopping. False for `left`, and for a file with no
    /// state at all -- written before 2026-10-16, or by hand -- which says
    /// nothing either way.
    pub abandoned: bool,
}

impl Marker {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            path: dir.join(FILE_NAME),
        }
    }

    /// The marker where it belongs on this machine, or `None` when Windows
    /// offers no local profile -- in which case there is no log either.
    pub fn in_local_dir() -> Option<Self> {
        crate::config::local_dir().map(|dir| Self::in_dir(&dir))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Record that a session is open. Overwrites, so a rename mid-session
    /// keeps the file current.
    pub fn open(&self, game: Option<&str>, since: &str) -> io::Result<()> {
        self.write(game, Some(since), OPEN)
    }

    /// A watcher takes up the session the file holds, name and time
    /// unchanged.
    pub fn hold(&self) -> io::Result<()> {
        self.restate(OPEN)
    }

    /// The watcher stops and leaves the session to the next start, name and
    /// time unchanged.
    pub fn leave(&self) -> io::Result<()> {
        self.restate(LEFT)
    }

    fn restate(&self, state: &str) -> io::Result<()> {
        let pending = self.pending().unwrap_or_default();
        self.write(pending.game.as_deref(), pending.since.as_deref(), state)
    }

    fn write(&self, game: Option<&str>, since: Option<&str>, state: &str) -> io::Result<()> {
        let mut text = String::from(
            "# GameModeExecutor: a game session is open and its stop commands have not run.\n\
             # If this file is still here when the watcher starts, they run then.\n\
             # state: open while a watcher holds the session, left once one stopped\n\
             # and left it to the next start.\n",
        );
        if let Some(game) = game {
            text.push_str(&format!("game = {game}\n"));
        }
        if let Some(since) = since {
            text.push_str(&format!("since = {since}\n"));
        }
        text.push_str(&format!("state = {state}\n"));
        // The folder may not exist yet: the log can be configured elsewhere,
        // and then nothing else has had a reason to create it.
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&self.path, text)
    }

    /// The session closed. A file that is already gone is not an error.
    pub fn close(&self) -> io::Result<()> {
        match fs::remove_file(&self.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    /// What the last run left behind, if anything.
    pub fn pending(&self) -> Option<Pending> {
        let text = fs::read_to_string(&self.path).ok()?;
        Some(parse(&text))
    }
}

/// "The configuration could not be used when the watcher last looked":
/// presence is the signal, the contents are for whoever opens the file.
#[derive(Clone)]
pub struct FaultMarker {
    path: PathBuf,
}

impl FaultMarker {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            path: dir.join(FAULT_FILE_NAME),
        }
    }

    /// Beside the session marker, or `None` when Windows offers no local
    /// profile.
    pub fn in_local_dir() -> Option<Self> {
        crate::config::local_dir().map(|dir| Self::in_dir(&dir))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Record a fault. Overwrites, so the file says the latest one.
    pub fn note(&self, summary: &str, since: &str) -> io::Result<()> {
        let text = format!(
            "# GameModeExecutor: the configuration could not be used, so nothing is watched.\n\
             # Removed by the watcher once a usable file is read.\n\
             fault = {summary}\nsince = {since}\n"
        );
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&self.path, text)
    }

    /// The configuration is usable: forget the fault. Says whether there
    /// was one to forget, which is what a start needs to know.
    pub fn clear(&self) -> io::Result<bool> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}

/// Lenient on purpose: the file is written by this program, but a person may
/// have opened it, and its presence matters more than its contents.
fn parse(text: &str) -> Pending {
    let mut pending = Pending::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "game" if !value.is_empty() => pending.game = Some(value.to_owned()),
            "since" if !value.is_empty() => pending.since = Some(value.to_owned()),
            "state" => pending.abandoned = value == OPEN,
            _ => {}
        }
    }
    pending
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "gamemode-executor-marker-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A fault is remembered across processes: noted, then cleared once,
    /// and the clearing says whether there was anything to clear.
    #[test]
    fn a_fault_is_remembered_until_a_usable_file_clears_it() {
        let marker = FaultMarker::in_dir(&scratch().join("fresh"));
        assert!(!marker.clear().unwrap(), "nothing to forget at first");
        marker.note("line 3: unknown field `x`", "now").unwrap();
        assert!(marker.path().is_file());
        let text = fs::read_to_string(marker.path()).unwrap();
        assert!(text.contains("fault = line 3: unknown field `x`"), "{text}");
        assert!(marker.clear().unwrap(), "there was a fault to forget");
        assert!(!marker.clear().unwrap(), "and only once");
    }

    #[test]
    fn opening_creates_the_folder_when_nothing_else_has() {
        // The log may be configured elsewhere, so the local folder can be
        // absent at the first game.
        let marker = Marker::in_dir(&scratch().join("not-yet-there"));
        marker.open(Some("game.exe"), "t0").unwrap();
        assert!(marker.pending().is_some());
    }

    #[test]
    fn nothing_pending_when_no_session_was_open() {
        let marker = Marker::in_dir(&scratch());
        assert_eq!(marker.pending(), None);
    }

    #[test]
    fn an_open_session_is_found_again_with_its_name() {
        let marker = Marker::in_dir(&scratch());
        marker
            .open(Some("Starfield.exe"), "2026-09-16 00:16:51")
            .unwrap();
        assert_eq!(
            marker.pending(),
            Some(Pending {
                game: Some("Starfield.exe".to_owned()),
                since: Some("2026-09-16 00:16:51".to_owned()),
                abandoned: true,
            })
        );
    }

    #[test]
    fn a_session_left_to_the_next_start_keeps_its_name_and_time() {
        // Found like this, the last watcher stopped in an orderly way.
        let marker = Marker::in_dir(&scratch());
        marker
            .open(Some("Starfield.exe"), "2026-09-16 00:16:51")
            .unwrap();
        marker.leave().unwrap();
        assert_eq!(
            marker.pending(),
            Some(Pending {
                game: Some("Starfield.exe".to_owned()),
                since: Some("2026-09-16 00:16:51".to_owned()),
                abandoned: false,
            })
        );
        // A watcher that takes it up holds it again: found like this by a
        // start, it was abandoned.
        marker.hold().unwrap();
        assert!(marker.pending().unwrap().abandoned);
    }

    #[test]
    fn a_marker_without_a_state_line_says_nothing_either_way() {
        // 0.3.0 wrote none, and an update hands such a file to this version.
        let marker = Marker::in_dir(&scratch());
        fs::write(marker.path(), "game = game.exe\nsince = t0\n").unwrap();
        let pending = marker.pending().unwrap();
        assert!(!pending.abandoned);
        assert_eq!(pending.game.as_deref(), Some("game.exe"));
    }

    #[test]
    fn a_closed_session_leaves_nothing_behind() {
        let marker = Marker::in_dir(&scratch());
        marker.open(None, "2026-09-16 00:16:51").unwrap();
        marker.close().unwrap();
        assert_eq!(marker.pending(), None);
    }

    #[test]
    fn closing_twice_is_not_an_error() {
        // The normal stop closes it, and so does the stop that follows a
        // recovery; neither should fail because the other went first.
        let marker = Marker::in_dir(&scratch());
        marker.close().unwrap();
        marker.close().unwrap();
    }

    #[test]
    fn a_session_without_a_name_is_still_pending() {
        // Windows tracked a game it did not name. Recovery must still run.
        let marker = Marker::in_dir(&scratch());
        marker.open(None, "2026-09-16 00:16:51").unwrap();
        let pending = marker.pending().unwrap();
        assert_eq!(pending.game, None);
        assert_eq!(pending.since.as_deref(), Some("2026-09-16 00:16:51"));
    }

    #[test]
    fn a_reopened_session_reports_the_newer_name() {
        // The refinement renamed the session; the file should say so.
        let marker = Marker::in_dir(&scratch());
        marker.open(Some("gamelaunchhelper.exe"), "t0").unwrap();
        marker.open(Some("Starfield.exe"), "t0").unwrap();
        assert_eq!(
            marker.pending().unwrap().game.as_deref(),
            Some("Starfield.exe")
        );
    }

    #[test]
    fn a_hand_edited_file_still_counts() {
        let marker = Marker::in_dir(&scratch());
        fs::write(marker.path(), "someone opened this in notepad\n").unwrap();
        assert_eq!(marker.pending(), Some(Pending::default()));
    }
}
