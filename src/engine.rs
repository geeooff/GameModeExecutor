//! The watcher loop.
//!
//! Detection is delegated entirely to Windows. The Game Bar presence writer is
//! an on-demand WinRT server that Windows activates for a game and releases
//! when the game is gone, and its process was measured to appear 7 ms after
//! activation and exit within 20 ms of release. So its lifetime *is* the game
//! session, and the loop is:
//!
//! - idle: look for the writer process every `poll_interval`;
//! - active: park on the writer's process handle, so nothing runs at all until
//!   Windows lets it go.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::actions::{self, ActionContext};
use crate::config::Config;
use crate::detect::GameSignal;
use crate::detect::known_games::KnownGames;
use crate::detect::presence_writer::{self, WaitOutcome};
use crate::detect::process::Snapshot;
use crate::win::StopSignal;

pub struct Engine {
    config: Config,
    /// Resolved from the registry once at startup, never hard-coded.
    writer_exe: PathBuf,
}

impl Engine {
    pub fn new(config: Config) -> Result<Self> {
        let writer_exe = presence_writer::registered_exe()?;
        Ok(Self { config, writer_exe })
    }

    pub fn writer_exe(&self) -> &Path {
        &self.writer_exe
    }

    pub fn run(&mut self, stop: &StopSignal) -> Result<()> {
        tracing::info!(
            "watching {} (idle poll {:?})",
            self.writer_exe.display(),
            self.config.detection.poll_interval
        );
        if !presence_writer::is_microsoft_default(&self.writer_exe) {
            tracing::warn!(
                "the presence writer registration is not the Microsoft default; \
                 detection follows whatever is registered"
            );
        }

        while let Some(mut pid) = self.await_writer(stop) {
            let signal = self.identify();
            self.fire_start(signal.as_ref());

            // Stay active across a brief writer restart, so a game that makes
            // Windows re-activate it does not flap the actions.
            let stopped = loop {
                match presence_writer::wait_for_exit(pid, stop)? {
                    WaitOutcome::Stopped => break true,
                    WaitOutcome::WriterExited => {}
                }
                match self.writer_returns(stop) {
                    Some(new_pid) => {
                        tracing::debug!(
                            "presence writer restarted as pid {new_pid}, still playing"
                        );
                        pid = new_pid;
                    }
                    None => break false,
                }
            };

            if stopped {
                if self.config.general.stop_actions_on_exit {
                    tracing::info!("shutting down while a game is running");
                    self.fire_stop(signal.as_ref());
                }
                return Ok(());
            }
            self.fire_stop(signal.as_ref());
        }
        Ok(())
    }

    /// Idle: the only polling in the program. Returns `None` when stopped.
    fn await_writer(&self, stop: &StopSignal) -> Option<u32> {
        loop {
            if stop.is_set() {
                return None;
            }
            if let Some(pid) = presence_writer::running_pid(&self.writer_exe) {
                return Some(pid);
            }
            if stop.wait_timeout(self.config.detection.poll_interval) {
                return None;
            }
        }
    }

    /// After the writer exits, give it `stop_delay` to come back before
    /// declaring the session over.
    fn writer_returns(&self, stop: &StopSignal) -> Option<u32> {
        let grace = self.config.detection.stop_delay;
        if grace.is_zero() {
            return None;
        }
        let deadline = std::time::Instant::now() + grace;
        while std::time::Instant::now() < deadline {
            if stop.wait_timeout(self.config.detection.poll_interval.min(grace)) {
                return None;
            }
            if let Some(pid) = presence_writer::running_pid(&self.writer_exe) {
                return Some(pid);
            }
        }
        None
    }

    /// Put a name on the game Windows just flagged, for the logs and the
    /// action placeholders. Detection does not depend on this working.
    fn identify(&self) -> Option<GameSignal> {
        let known = match KnownGames::load() {
            Ok(known) => known,
            Err(error) => {
                tracing::warn!("cannot read the known game list: {error:#}");
                return None;
            }
        };
        let snapshot = match Snapshot::take() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!("cannot enumerate processes: {error:#}");
                return None;
            }
        };
        let signal = known.identify(&snapshot);
        if signal.is_none() {
            tracing::debug!("a game is running but no known game list entry matches it");
        }
        signal
    }

    /// Manual trigger, used by the `trigger start` command.
    pub fn fire_start_manual(&self) {
        actions::run_all(
            &self.config.on_game_start,
            &ActionContext::new("game_start", None),
        );
    }

    fn fire_start(&self, signal: Option<&GameSignal>) {
        match signal {
            Some(signal) => tracing::info!("game started: {}", signal.describe()),
            None => tracing::info!("game started (unidentified)"),
        }
        actions::run_all(
            &self.config.on_game_start,
            &ActionContext::new("game_start", signal),
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
