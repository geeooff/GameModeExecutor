//! Game detection.
//!
//! Only the shell full-screen detector is left for now: the process allow-list
//! detector was removed in favour of relying on Windows' own game detection.

pub mod fullscreen;
pub mod process;

use anyhow::Result;

use crate::config::Detection;

use fullscreen::FullscreenDetector;
use process::Snapshot;

/// What a detector saw. Fields other than `source` are best effort and feed
/// the placeholders available to actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSignal {
    pub source: &'static str,
    pub process_name: Option<String>,
    pub process_id: Option<u32>,
    pub process_path: Option<String>,
}

impl GameSignal {
    pub fn describe(&self) -> String {
        match (&self.process_name, self.process_id) {
            (Some(name), Some(pid)) => format!("{name} (pid {pid}, via {})", self.source),
            (Some(name), None) => format!("{name} (via {})", self.source),
            _ => format!("unknown process (via {})", self.source),
        }
    }
}

pub struct Detectors {
    fullscreen: Option<FullscreenDetector>,
}

impl Detectors {
    pub fn new(detection: &Detection) -> Self {
        Self {
            fullscreen: detection
                .fullscreen
                .enabled
                .then(|| FullscreenDetector::new(&detection.fullscreen.states)),
        }
    }

    /// Errors from a detector are logged and treated as "no game".
    pub fn detect(&self, snapshot: &Snapshot) -> Option<GameSignal> {
        let detector = self.fullscreen.as_ref()?;
        match detector.detect(snapshot) {
            Ok(signal) => signal,
            Err(error) => {
                tracing::warn!("fullscreen detector failed: {error:#}");
                None
            }
        }
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        Snapshot::take()
    }
}
