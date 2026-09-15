//! A file that says "a game session is open and its stop commands have not run".
//!
//! Written when the start commands run, removed once the stop commands are
//! confirmed. Found at the next start, it means the last session never closed
//! -- a logoff, a shutdown, a crash, a power cut -- and the stop commands run
//! then instead.
//!
//! This exists because of a measurement, not a hypothetical. A real logoff on
//! 2026-09-16 showed that a process started even one millisecond after
//! `WM_QUERYENDSESSION` dies with `STATUS_DLL_INIT_FAILED`: the session-end
//! handshake works, and the command it starts is stillborn. So the stop
//! commands cannot run at session end, and the only moment that does not
//! depend on Windows' timing is the next start.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Named so that someone finding it in the folder needs no documentation.
pub const FILE_NAME: &str = "pending-stop-actions.txt";

pub struct Marker {
    path: PathBuf,
}

/// What a marker left behind says about the session it belonged to. Both
/// fields are informational; the file's presence is the signal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pending {
    pub game: Option<String>,
    pub since: Option<String>,
}

impl Marker {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            path: dir.join(FILE_NAME),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Record that a session is open. Overwrites, so a rename mid-session
    /// keeps the file current.
    pub fn open(&self, game: Option<&str>, since: &str) -> io::Result<()> {
        let mut text = String::from(
            "# GameModeExecutor: a game session is open and its stop commands have not run.\n\
             # If this file is still here when the watcher starts, they run then.\n",
        );
        if let Some(game) = game {
            text.push_str(&format!("game = {game}\n"));
        }
        text.push_str(&format!("since = {since}\n"));
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
            })
        );
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
