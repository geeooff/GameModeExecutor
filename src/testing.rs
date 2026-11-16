//! What the unit tests share.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

/// An empty folder under the temporary directory, its own within this run.
///
/// Emptied first: the name carries the test process's id, which Windows
/// reuses, and a folder an earlier run left under the same id once handed a
/// scenario that run's session marker (2026-10-17). Before anything is
/// removed, the folder is checked to be a direct child of the temporary
/// directory: `join` discards what comes before an absolute path, so a
/// prefix that were one would point the removal elsewhere.
pub fn fresh_dir(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = std::env::temp_dir();
    let dir = temp.join(format!("{prefix}-{}-{n}", std::process::id()));
    assert_eq!(
        dir.parent(),
        Some(temp.as_path()),
        "a scratch folder outside the temporary directory: {}",
        dir.display()
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
