//! What the unit tests share.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

/// An empty folder under the temporary directory, its own within this run.
///
/// Emptied first: the name carries the test process's id, which Windows
/// reuses, and a folder an earlier run left under the same id once handed a
/// scenario that run's session marker (2026-10-17).
pub fn fresh_dir(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
