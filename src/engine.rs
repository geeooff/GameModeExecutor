//! The polling loop and its debounced state machine.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::actions::{self, ActionContext};
use crate::config::Config;
use crate::detect::{Detectors, GameSignal};

#[derive(Debug, Clone)]
enum Phase {
    /// No game seen.
    Idle,
    /// A game was seen, waiting out start_delay.
    Arming { since: Instant, signal: GameSignal },
    /// on_game_start has run.
    Active { signal: GameSignal },
    /// The game disappeared, waiting out stop_delay.
    Disarming { since: Instant, signal: GameSignal },
}

pub struct Engine {
    config: Config,
    detectors: Detectors,
    phase: Phase,
}

impl Engine {
    pub fn new(config: Config) -> Self {
        let detectors = Detectors::new(&config.detection);
        Self {
            config,
            detectors,
            phase: Phase::Idle,
        }
    }

    /// Poll until stop is set, then optionally run the stop actions.
    pub fn run(&mut self, stop: Arc<AtomicBool>) -> Result<()> {
        tracing::info!(
            "watching every {:?} (start delay {:?}, stop delay {:?})",
            self.config.general.poll_interval,
            self.config.general.start_delay,
            self.config.general.stop_delay
        );

        while !stop.load(Ordering::Relaxed) {
            if let Err(error) = self.tick() {
                tracing::error!("detection tick failed: {error:#}");
            }
            // Sleep in slices so a shutdown request is picked up quickly.
            let deadline = Instant::now() + self.config.general.poll_interval;
            while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        if self.config.general.stop_actions_on_exit
            && let Phase::Active { signal } | Phase::Disarming { signal, .. } = self.phase.clone()
        {
            tracing::info!("shutting down while a game is active, running stop actions");
            self.fire_stop(Some(&signal));
        }
        Ok(())
    }

    fn tick(&mut self) -> Result<()> {
        let snapshot = self.detectors.snapshot()?;
        let detected = self.detectors.detect(&snapshot);
        let now = Instant::now();

        self.phase = match (self.phase.clone(), detected) {
            (Phase::Idle, Some(signal)) => {
                if self.config.general.start_delay.is_zero() {
                    self.fire_start(&signal);
                    Phase::Active { signal }
                } else {
                    tracing::debug!("game candidate: {}", signal.describe());
                    Phase::Arming { since: now, signal }
                }
            }
            (Phase::Idle, None) => Phase::Idle,

            (Phase::Arming { since, .. }, Some(signal)) => {
                if now.duration_since(since) >= self.config.general.start_delay {
                    self.fire_start(&signal);
                    Phase::Active { signal }
                } else {
                    Phase::Arming { since, signal }
                }
            }
            (Phase::Arming { signal, .. }, None) => {
                tracing::debug!(
                    "candidate {} vanished before start delay",
                    signal.describe()
                );
                Phase::Idle
            }

            (Phase::Active { .. }, Some(signal)) => Phase::Active { signal },
            (Phase::Active { signal }, None) => {
                if self.config.general.stop_delay.is_zero() {
                    self.fire_stop(Some(&signal));
                    Phase::Idle
                } else {
                    tracing::debug!("{} vanished, waiting stop delay", signal.describe());
                    Phase::Disarming { since: now, signal }
                }
            }

            (Phase::Disarming { .. }, Some(signal)) => {
                tracing::debug!("game came back: {}", signal.describe());
                Phase::Active { signal }
            }
            (Phase::Disarming { since, signal }, None) => {
                if now.duration_since(since) >= self.config.general.stop_delay {
                    self.fire_stop(Some(&signal));
                    Phase::Idle
                } else {
                    Phase::Disarming { since, signal }
                }
            }
        };
        Ok(())
    }

    /// Manual trigger, used by the `trigger start` command.
    pub fn fire_start_manual(&self) {
        actions::run_all(
            &self.config.on_game_start,
            &ActionContext::new("game_start", None),
        );
    }

    fn fire_start(&self, signal: &GameSignal) {
        tracing::info!("game started: {}", signal.describe());
        actions::run_all(
            &self.config.on_game_start,
            &ActionContext::new("game_start", Some(signal)),
        );
    }

    pub fn fire_stop(&self, signal: Option<&GameSignal>) {
        match signal {
            Some(signal) => tracing::info!("game stopped: {}", signal.describe()),
            None => tracing::info!("game stopped"),
        }
        actions::run_all(
            &self.config.on_game_stop,
            &ActionContext::new("game_stop", signal),
        );
    }
}
