//! Game detection, delegated to Windows.
//!
//! `presence_writer` is the detector: the lifetime of the Game Bar presence
//! writer process is the game session. `known_games` only puts a name on what
//! it found, and `fullscreen` is kept for diagnostics.

pub mod fullscreen;
pub mod gpu;
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

/// Among processes the known game list matched, pick the one that is actually
/// rendering.
///
/// This only orders candidates the list already produced; it never promotes a
/// process the list did not match. When nothing has measurable load — a game
/// still on its loading screen, or counters we may not read — the first
/// candidate is kept, which is what the caller would have used anyway.
pub fn most_active(
    candidates: Vec<GameSignal>,
    load: &std::collections::HashMap<u32, f64>,
) -> Option<GameSignal> {
    let load_of = |signal: &GameSignal| {
        signal
            .process_id
            .and_then(|pid| load.get(&pid))
            .copied()
            .unwrap_or(0.0)
    };
    // Strictly greater, so equal loads keep the earlier candidate: with no
    // measurement to separate them this is the one the caller would have used.
    candidates.into_iter().reduce(|best, next| {
        if load_of(&next) > load_of(&best) {
            next
        } else {
            best
        }
    })
}

impl GameSignal {
    /// Just the name, for a log line someone reads without wanting to know
    /// what a pid is. The id and how it was matched go in the event's fields,
    /// where the level decides whether they are shown.
    pub fn name(&self) -> &str {
        self.process_name
            .as_deref()
            .unwrap_or("an unrecognised process")
    }

    /// Name and annotations in one string, for `status` and other places that
    /// print rather than log.
    pub fn describe(&self) -> String {
        match (&self.process_name, self.process_id) {
            (Some(name), Some(pid)) => format!("{name} (pid {pid}, via {})", self.source),
            (Some(name), None) => format!("{name} (via {})", self.source),
            _ => format!("unknown process (via {})", self.source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn signal(pid: u32, name: &str) -> GameSignal {
        GameSignal {
            source: "test",
            process_name: Some(name.to_owned()),
            process_id: Some(pid),
            process_path: None,
        }
    }

    #[test]
    fn the_rendering_process_wins_over_a_launcher_stub() {
        // The shape of a real session: the stub matched first, the game is
        // what the GPU is busy with.
        let candidates = vec![
            signal(1, "gamelaunchhelper.exe"),
            signal(2, "Starfield.exe"),
        ];
        let load = HashMap::from([(2, 87.5)]);
        let best = most_active(candidates, &load).unwrap();
        assert_eq!(best.process_name.as_deref(), Some("Starfield.exe"));
    }

    #[test]
    fn an_anti_cheat_service_does_not_outrank_the_game() {
        let candidates = vec![
            signal(10, "EAAntiCheat.GameServiceLauncher.exe"),
            signal(11, "bf6.exe"),
        ];
        let load = HashMap::from([(10, 0.4), (11, 63.0)]);
        assert_eq!(most_active(candidates, &load).unwrap().process_id, Some(11));
    }

    #[test]
    fn without_any_load_the_first_candidate_is_kept() {
        // A game still loading, or counters this account cannot read.
        let candidates = vec![signal(1, "first.exe"), signal(2, "second.exe")];
        assert_eq!(
            most_active(candidates, &HashMap::new()).unwrap().process_id,
            Some(1)
        );
    }

    #[test]
    fn nothing_matched_stays_nothing() {
        assert!(most_active(Vec::new(), &HashMap::new()).is_none());
    }
}
