//! Game detection, delegated to Windows.
//!
//! `presence_writer` is the detector: the lifetime of the Game Bar presence
//! writer process is the game session. `known_games` only puts a name on what
//! it found, and `fullscreen` is kept for diagnostics.

pub mod fullscreen;
pub mod known_games;
pub mod presence_writer;
pub mod process;

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
