//! Game detection: several detectors sharing one process snapshot per tick.

pub mod fullscreen;
pub mod process;

use anyhow::Result;

use crate::config::{Detection, MatchMode};

use fullscreen::FullscreenDetector;
use process::{ProcessDetector, Snapshot, normalize_name};

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
    match_mode: MatchMode,
    ignored: Vec<String>,
    process: Option<ProcessDetector>,
    fullscreen: Option<FullscreenDetector>,
}

impl Detectors {
    pub fn new(detection: &Detection) -> Self {
        Self {
            match_mode: detection.match_mode,
            ignored: detection
                .ignore_processes
                .iter()
                .map(|name| normalize_name(name))
                .collect(),
            process: (!detection.processes.is_empty())
                .then(|| ProcessDetector::new(&detection.processes)),
            fullscreen: detection
                .fullscreen
                .enabled
                .then(|| FullscreenDetector::new(&detection.fullscreen.states)),
        }
    }

    /// Run every enabled detector and combine the results according to the
    /// configured match mode. Errors from a single detector are logged and
    /// treated as "no game".
    pub fn detect(&self, snapshot: &Snapshot) -> Option<GameSignal> {
        let mut signals = Vec::new();
        let mut enabled = 0usize;

        if let Some(detector) = &self.process {
            enabled += 1;
            if let Some(signal) = detector.detect(snapshot) {
                signals.push(signal);
            }
        }
        if let Some(detector) = &self.fullscreen {
            enabled += 1;
            match detector.detect(snapshot) {
                Ok(Some(signal)) => signals.push(signal),
                Ok(None) => {}
                Err(error) => tracing::warn!("fullscreen detector failed: {error:#}"),
            }
        }

        let matched = match self.match_mode {
            MatchMode::Any => !signals.is_empty(),
            MatchMode::All => enabled > 0 && signals.len() == enabled,
        };
        if !matched {
            return None;
        }

        // Prefer a signal that actually names a process, for nicer logs and
        // placeholders.
        let signal = signals
            .iter()
            .find(|signal| signal.process_name.is_some())
            .or_else(|| signals.first())?
            .clone();

        if let Some(name) = &signal.process_name
            && self.ignored.contains(&normalize_name(name))
        {
            tracing::debug!("ignoring {name}: listed in detection.ignore_processes");
            return None;
        }
        Some(signal)
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        Snapshot::take()
    }
}
