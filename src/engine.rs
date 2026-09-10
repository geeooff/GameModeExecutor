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
use crate::detect::known_games::KnownGames;
use crate::detect::presence_writer::{self, WaitOutcome};
use crate::detect::process::Snapshot;
use crate::detect::{self, GameSignal, gpu};
use crate::logging::target;
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
        tracing::debug!(
            target: target::WATCHER,
            writer = %self.writer_exe.display(),
            idle_poll = ?self.config.detection.poll_interval,
            "Watching for games"
        );
        if !presence_writer::is_microsoft_default(&self.writer_exe) {
            tracing::warn!(
                target: target::WATCHER,
                writer = %self.writer_exe.display(),
                "The registered Game Bar presence writer is not the one Windows ships; \
                 detection follows whatever is registered"
            );
        }

        while let Some(mut pid) = self.await_writer(stop) {
            let session_start = std::time::Instant::now();
            let mut signal = self.identify();
            self.fire_start(signal.as_ref());

            // The satellites of a title -- launcher stubs, anti-cheat services
            // -- match the known game list too and usually start first, so the
            // name captured a moment ago is often the wrong one. Once, a little
            // way into the session, ask which candidate is actually rendering.
            let mut refine_due = !self.config.detection.identify_after.is_zero();

            let stopped = loop {
                let timeout = refine_due.then_some(self.config.detection.identify_after);
                // Waiting on the writer's handle rather than sleeping keeps the
                // refinement from being blind to a game ending in the meantime.
                match presence_writer::wait_for_exit_until(pid, stop, timeout)? {
                    WaitOutcome::Stopped => break true,
                    WaitOutcome::TimedOut => {
                        refine_due = false;
                        if let Some(better) = self.refine(signal.as_ref()) {
                            signal = Some(better);
                        }
                        continue;
                    }
                    WaitOutcome::WriterExited => {
                        self.log_writer_exit(session_start, signal.as_ref());
                    }
                }
                match self.writer_returns(stop) {
                    Some(new_pid) => {
                        tracing::debug!(
                            target: target::GAME,
                            pid = new_pid,
                            "Presence writer came back, the session is still running"
                        );
                        pid = new_pid;
                    }
                    None => break false,
                }
            };

            if stopped {
                if self.config.general.stop_actions_on_exit {
                    // Stays at info: without it the reader sees a session end
                    // and has no way to tell the game stopped from the watcher
                    // stopping under it.
                    tracing::info!(
                        target: target::WATCHER,
                        "Stopping while a game is running, so the stop commands run now"
                    );
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
        loop {
            // Wait the shorter of a poll and what is left, so the grace period
            // is honoured to the configured value rather than rounded up to a
            // whole number of polls.
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            if stop.wait_timeout(self.config.detection.poll_interval.min(remaining)) {
                return None;
            }
            if let Some(pid) = presence_writer::running_pid(&self.writer_exe) {
                return Some(pid);
            }
        }
    }

    /// Record what the writer's exit actually means.
    ///
    /// Windows releases the presence writer well after the game process dies,
    /// and by an amount that is not a per-title constant: one BF6 session was
    /// measured at 2 min 4 s, an earlier one at seconds. Without this line the
    /// log jumps straight from the start to the stop, and telling "Windows was
    /// slow" from "we were slow" needs Steam's own logs. So say whether the
    /// game we identified was already gone when Windows finally let go.
    fn log_writer_exit(&self, session_start: std::time::Instant, signal: Option<&GameSignal>) {
        let elapsed = session_start.elapsed();
        let named = signal.and_then(|signal| signal.process_id).map(|pid| {
            let alive = Snapshot::take()
                .ok()
                .is_some_and(|snapshot| snapshot.by_pid(pid).is_some());
            (pid, alive)
        });
        match named {
            Some((pid, true)) => tracing::debug!(
                target: target::GAME,
                pid,
                session = ?elapsed,
                "Windows released the presence writer while the identified game is still running"
            ),
            Some((pid, false)) => tracing::debug!(
                target: target::GAME,
                pid,
                session = ?elapsed,
                "Windows released the presence writer; the identified game had already \
                 exited, so the wait since then was Windows, not this program"
            ),
            None => tracing::debug!(
                target: target::GAME,
                session = ?elapsed,
                "Windows released the presence writer; the game was never named"
            ),
        }
    }

    /// Put a name on the game Windows just flagged, for the logs and the
    /// action placeholders. Detection does not depend on this working.
    fn identify(&self) -> Option<GameSignal> {
        let known = match KnownGames::load() {
            Ok(known) => known,
            Err(error) => {
                tracing::warn!(target: target::GAME, error = %format!("{error:#}"), "Cannot read Windows' known game list, so the game cannot be named");
                return None;
            }
        };
        let snapshot = match Snapshot::take() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(target: target::GAME, error = %format!("{error:#}"), "Cannot list running processes, so the game cannot be named");
                return None;
            }
        };
        known.identify(&snapshot)
    }

    /// Ask the GPU which of the matched processes is really the game.
    ///
    /// Returns `None` when there is nothing better to say, which covers a
    /// single candidate, counters this account may not read, and a game still
    /// on its loading screen. Naming is a convenience: no answer is a fine
    /// answer.
    fn refine(&self, current: Option<&GameSignal>) -> Option<GameSignal> {
        let known = match KnownGames::load() {
            Ok(known) => known,
            Err(error) => {
                tracing::debug!(target: target::GAME, error = %format!("{error:#}"), "Refinement skipped, cannot read the known game list");
                return None;
            }
        };
        let snapshot = match Snapshot::take() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::debug!(target: target::GAME, error = %format!("{error:#}"), "Refinement skipped, cannot list running processes");
                return None;
            }
        };
        let candidates = known.candidates(&snapshot);
        if candidates.len() < 2 {
            tracing::debug!(
                target: target::GAME,
                candidates = candidates.len(),
                "Refinement has nothing to arbitrate, keeping the current name"
            );
            return None;
        }

        let load = match gpu::rendering_load(self.config.detection.gpu_sample) {
            Ok(load) => load,
            Err(error) => {
                tracing::debug!(target: target::GAME, error = %format!("{error:#}"), "Cannot read the GPU counters, keeping the first match");
                return None;
            }
        };

        let best = detect::most_active(candidates, &load)?;
        let share = best
            .process_id
            .and_then(|pid| load.get(&pid))
            .copied()
            .unwrap_or(0.0);
        // Checked before comparing with the current name, so that confirming a
        // name can quote the share that confirms it.
        if share <= 0.0 {
            tracing::debug!(
                target: target::GAME,
                "None of the matched processes is rendering yet, keeping the current name"
            );
            return None;
        }
        if current.and_then(|signal| signal.process_id) == best.process_id {
            tracing::debug!(
                target: target::GAME,
                pid = best.process_id,
                rendering_share = share,
                "The GPU confirms the name already in use: {}",
                best.name()
            );
            return None;
        }
        tracing::info!(
            target: target::GAME,
            pid = best.process_id,
            matched_by = best.source,
            rendering_share = share,
            "Game identified more precisely: {} ({share:.0}% of the rendering)",
            best.name()
        );
        Some(best)
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
            Some(signal) => tracing::info!(
                target: target::GAME,
                pid = signal.process_id,
                matched_by = signal.source,
                path = signal.process_path.as_deref(),
                "Game detected: {}",
                signal.name()
            ),
            // Naming is a convenience; not managing it changes nothing about
            // detection, so say exactly that rather than looking like a failure.
            None => tracing::info!(
                target: target::GAME,
                "Game detected, but Windows does not name this one. The commands still \
                 run, with the name placeholders empty"
            ),
        }
        actions::run_all(
            &self.config.on_game_start,
            &ActionContext::new("game_start", signal),
        );
    }

    pub fn fire_stop(&self, signal: Option<&GameSignal>) {
        // Deliberately no process id here. The name was captured when the
        // session started; by now that process is usually long gone, and a
        // satellite of the real game as often as not. Reporting the id would
        // assert something we cannot vouch for.
        match signal.and_then(|signal| signal.process_name.as_deref()) {
            Some(name) => tracing::info!(
                target: target::GAME,
                "Game no longer detected: {name}"
            ),
            None => tracing::info!(
                target: target::GAME,
                "Game no longer detected, it was never named"
            ),
        }
        actions::run_all(
            &self.config.on_game_stop,
            &ActionContext::new("game_stop", signal),
        );
    }
}
