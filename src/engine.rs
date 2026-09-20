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
//!
//! The engine decides and never reads the OS itself: everything it observes
//! comes through a [`Sensor`], which is what lets `engine/tests.rs` drive whole
//! sessions in milliseconds with a scripted one.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;

use crate::actions::{self, ActionContext, Outcome};
use crate::config::Config;
use crate::detect::presence_writer::WaitOutcome;
use crate::detect::{self, GameSignal};
use crate::logging::{self, target};
use crate::marker::Marker;
use crate::sensor::Sensor;
use crate::win::{StopReason, StopSignal};

/// What the engine tells the outside world about the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Session {
    /// No game.
    Idle,
    /// A game is running. Named when the Known Game List matched a process;
    /// `None` is a game Windows tracks but does not describe, which is a
    /// session all the same.
    Playing(Option<GameSignal>),
}

/// Told whenever the session changes: a game started, was named more
/// precisely, or ended.
///
/// A callback rather than the engine knowing about the tray. The engine has
/// no business knowing that anything is drawn anywhere; the caller decides
/// what a change means. Without one the engine behaves exactly as before.
pub type SessionSink = Arc<dyn Fn(&Session) + Send + Sync>;

pub struct Engine<S: Sensor> {
    config: Config,
    sensor: S,
    session: Option<SessionSink>,
    /// Where "a session is open" is remembered across the process's death.
    /// Without one the engine forgets everything when it exits, which is what
    /// most tests want and what a logoff cannot afford.
    marker: Option<Marker>,
}

impl<S: Sensor> Engine<S> {
    pub fn new(config: Config, sensor: S) -> Self {
        Self {
            config,
            sensor,
            session: None,
            marker: None,
        }
    }

    /// Report session changes to `sink` as well as to the log.
    #[must_use]
    pub fn reporting_to(mut self, sink: SessionSink) -> Self {
        self.session = Some(sink);
        self
    }

    /// Remember an open session in `marker`, so that a session this process
    /// does not live to close is closed by the next one.
    #[must_use]
    pub fn remembering(mut self, marker: Marker) -> Self {
        self.marker = Some(marker);
        self
    }

    fn report(&self, session: &Session) {
        if let Some(sink) = &self.session {
            sink(session);
        }
    }

    /// Write the marker. Failing to is worth a warning and nothing more: the
    /// session goes on, only its recovery after a logoff is lost.
    fn remember(&self, signal: Option<&GameSignal>) {
        let Some(marker) = &self.marker else {
            return;
        };
        let game = signal.and_then(|signal| signal.process_name.as_deref());
        if let Err(error) = marker.open(game, &logging::local_now()) {
            tracing::warn!(
                target: target::WATCHER,
                path = %marker.path().display(),
                error = %error,
                "Cannot write the session marker, so a logoff during this game will \
                 not be recovered at the next start"
            );
        }
    }

    /// Remove the marker: the session is closed, or the user has opted out of
    /// closing it.
    fn forget(&self) {
        let Some(marker) = &self.marker else {
            return;
        };
        if let Err(error) = marker.close() {
            tracing::warn!(
                target: target::WATCHER,
                path = %marker.path().display(),
                error = %error,
                "Cannot remove the session marker, so the stop commands will run \
                 again at the next start"
            );
        }
    }

    /// Settle the session the last process left open: resume it when the
    /// game is still on, close it when the game is gone.
    ///
    /// Measured on 2026-09-16: a command started at logoff, even one
    /// millisecond after Windows first asks, dies with STATUS_DLL_INIT_FAILED.
    /// The session-end handshake is not where the stop commands can run, so
    /// they run here, at the start that follows. A logoff, a shutdown, a crash
    /// and a power cut are then one case.
    ///
    /// The other case, decided 2026-09-18: the writer is still running, so the
    /// game never ended -- the last watcher handed the session over for an
    /// update, the last engine of this process stopped for a reload, or a
    /// watcher crashed under it. Then nothing runs, neither stop nor start,
    /// and the session is taken up where it was. Looking for the writer
    /// *before* recovering is what keeps a game still on from getting the
    /// idle and then the gaming configuration seconds apart. Returns the
    /// writer to park on and the name to show when resuming.
    fn recover(&self) -> Option<(u32, Option<GameSignal>)> {
        let marker = self.marker.as_ref()?;
        let pending = marker.pending()?;
        if let Some(pid) = self.sensor.writer_pid() {
            let signal = pending.game.clone().map(|name| GameSignal {
                source: "resumed",
                process_name: Some(name),
                process_id: None,
                process_path: None,
            });
            match &pending.game {
                Some(game) => tracing::info!(
                    target: target::GAME,
                    since = pending.since.as_deref(),
                    "A session was left open with {game} still running, so it resumes where it was"
                ),
                None => tracing::info!(
                    target: target::GAME,
                    since = pending.since.as_deref(),
                    "A session was left open with a game still running, so it resumes where it was"
                ),
            }
            self.report(&Session::Playing(signal.clone()));
            return Some((pid, signal));
        }
        // The session is over. Said before the commands, as `fire_stop` does,
        // and said at all because the icon may still show the session the
        // engine before this one -- stopped for a reload -- left open.
        self.report(&Session::Idle);
        match &pending.game {
            Some(game) => tracing::info!(
                target: target::GAME,
                since = pending.since.as_deref(),
                "The last session ended with {game} still running and its stop commands \
                 never ran, so they run now"
            ),
            None => tracing::info!(
                target: target::GAME,
                since = pending.since.as_deref(),
                "The last session ended with a game still running and its stop commands \
                 never ran, so they run now"
            ),
        }
        let signal = pending.game.map(|name| GameSignal {
            source: "recovered",
            process_name: Some(name),
            process_id: None,
            process_path: None,
        });
        actions::run_all(
            &self.config.on_game_stop,
            &ActionContext::new("game_stop", signal.as_ref()),
        );
        self.forget();
        None
    }

    pub fn run(&mut self, stop: &StopSignal) -> Result<()> {
        tracing::debug!(
            target: target::WATCHER,
            idle_poll = ?self.config.detection.poll_interval,
            "Watching for games"
        );
        let mut resumed = self.recover();

        loop {
            // A resumed session already had its start: no commands, no
            // marker to write, and no refinement -- the name in the marker
            // is the refined one when there was one.
            let (mut pid, mut signal, fresh) = match resumed.take() {
                Some((pid, signal)) => (pid, signal, false),
                None => match self.await_writer(stop) {
                    Some(pid) => (pid, self.identify(), true),
                    None => break,
                },
            };
            let session_start = Instant::now();
            if fresh {
                self.fire_start(signal.as_ref());
            }

            // The satellites of a title -- launcher stubs, anti-cheat
            // services -- can match the known game list too, and the one that
            // matched first is not necessarily the one rendering: Battlefield 6
            // came up as its EA anti-cheat. Measured, that is the minority case.
            // Starfield and Skyrim each left a single candidate and this pass
            // found nothing to arbitrate. So: once, a little way into the
            // session, ask which candidate is actually rendering, and expect
            // "no better answer" more often than not.
            let mut refine_due = fresh && !self.config.detection.identify_after.is_zero();

            let stopped = loop {
                let timeout = refine_due.then_some(self.config.detection.identify_after);
                // Waiting on the writer's handle rather than sleeping keeps the
                // refinement from being blind to a game ending in the meantime.
                match self.sensor.wait_for_writer_exit(pid, stop, timeout)? {
                    WaitOutcome::Stopped => break true,
                    WaitOutcome::TimedOut => {
                        refine_due = false;
                        if let Some(better) = self.refine(signal.as_ref()) {
                            signal = Some(better);
                            // The name on screen was the launcher's until now.
                            self.report(&Session::Playing(signal.clone()));
                            self.remember(signal.as_ref());
                        }
                        continue;
                    }
                    WaitOutcome::WriterExited => {
                        self.log_writer_exit(session_start, signal.as_ref(), fresh);
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
                    // The writer's exit and the watcher's stop can arrive
                    // together. At logoff on 2026-09-17 Windows killed the
                    // writer 5 ms after asking the session to end, the wait
                    // reported the exit, and the ordinary path below removed
                    // the marker after a command that had died unborn. A stop
                    // that is set by now makes this the mid-game case,
                    // whichever of the two came first.
                    None => break stop.is_set(),
                }
            };

            if stopped {
                // A handover: the watcher that follows resumes this session,
                // so nothing runs and the marker stays open. The commands
                // would only have swapped the configuration twice in the
                // middle of a game. A reload is the same handover, to the
                // engine the supervisor builds next on the changed file.
                match stop.reason() {
                    StopReason::Handover => {
                        tracing::info!(
                            target: target::WATCHER,
                            "Stopping for an update; the game session is handed to the next watcher"
                        );
                        return Ok(());
                    }
                    StopReason::Reload => {
                        tracing::debug!(
                            target: target::WATCHER,
                            "Stopping for a reload; the game session is kept open for the next engine"
                        );
                        return Ok(());
                    }
                    StopReason::Restore => {}
                }
                if self.config.general.stop_actions_on_exit {
                    // Stays at info: without it the reader sees a session end
                    // and has no way to tell the game stopped from the watcher
                    // stopping under it.
                    tracing::info!(
                        target: target::WATCHER,
                        "Stopping while a game is running, so the stop commands run now"
                    );
                    // The marker outlives a stop that cannot be vouched for.
                    // At logoff the commands are started and die unborn; the
                    // next start is the only moment that is certain to have
                    // a working process to give them.
                    if self.fire_stop(signal.as_ref()).confirmed() {
                        self.forget();
                    } else {
                        tracing::warn!(
                            target: target::WATCHER,
                            "The stop commands could not be confirmed, so they run again at \
                             the next start"
                        );
                    }
                } else {
                    // Opted out of restoring on exit; that covers the next
                    // start too, or the opt-out would be undone at logon.
                    self.forget();
                }
                return Ok(());
            }
            // A game that stopped on its own: the commands are best effort,
            // as they always were, and the session is closed either way.
            self.fire_stop(signal.as_ref());
            self.forget();
        }
        Ok(())
    }

    /// Idle: the only polling in the program. Returns `None` when stopped.
    fn await_writer(&self, stop: &StopSignal) -> Option<u32> {
        loop {
            if stop.is_set() {
                return None;
            }
            if let Some(pid) = self.sensor.writer_pid() {
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
        let deadline = Instant::now() + grace;
        loop {
            // Wait the shorter of a poll and what is left, so the grace period
            // is honoured to the configured value rather than rounded up to a
            // whole number of polls.
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            if stop.wait_timeout(self.config.detection.poll_interval.min(remaining)) {
                return None;
            }
            if let Some(pid) = self.sensor.writer_pid() {
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
    ///
    /// A resumed session has a name from the marker and no process id, and
    /// this engine only saw the end of it: said as such, with the time since
    /// the resume rather than a session length it cannot know. Seen on
    /// 2026-09-20, when a session resumed after a reload was logged as
    /// never named, 34 s long.
    fn log_writer_exit(&self, session_start: Instant, signal: Option<&GameSignal>, fresh: bool) {
        let elapsed = session_start.elapsed();
        let named = signal
            .and_then(|signal| signal.process_id)
            .map(|pid| (pid, self.sensor.is_running(pid)));
        match (named, signal) {
            (Some((pid, true)), _) => tracing::debug!(
                target: target::GAME,
                pid,
                session = ?elapsed,
                "Windows released the presence writer while the identified game is still running"
            ),
            (Some((pid, false)), _) => tracing::debug!(
                target: target::GAME,
                pid,
                session = ?elapsed,
                "Windows released the presence writer; the identified game had already \
                 exited, so the wait since then was Windows, not this program"
            ),
            (None, Some(signal)) if !fresh => tracing::debug!(
                target: target::GAME,
                since_resumed = ?elapsed,
                "Windows released the presence writer; {} was known by name only, from the \
                 session this engine resumed, so whether it had already exited was not checked",
                signal.name()
            ),
            (None, _) => tracing::debug!(
                target: target::GAME,
                session = ?elapsed,
                "Windows released the presence writer; the game was never named"
            ),
        }
    }

    /// Put a name on the game Windows just flagged, for the logs and the
    /// action placeholders. Detection does not depend on this working.
    fn identify(&self) -> Option<GameSignal> {
        match self.sensor.candidates() {
            Ok(candidates) => candidates.into_iter().next(),
            Err(error) => {
                tracing::warn!(
                    target: target::GAME,
                    error = %format!("{error:#}"),
                    "The game cannot be named"
                );
                None
            }
        }
    }

    /// Ask the GPU which of the matched processes is really the game.
    ///
    /// Returns `None` when there is nothing better to say: counters this
    /// account may not read, a game still on its loading screen, or a single
    /// candidate that is already the name in use. A single candidate that is
    /// *not* the name in use is news, and needs no GPU to establish.
    /// Naming is a convenience: no answer is a fine answer.
    fn refine(&self, current: Option<&GameSignal>) -> Option<GameSignal> {
        let candidates = match self.sensor.candidates() {
            Ok(candidates) => candidates,
            Err(error) => {
                tracing::debug!(
                    target: target::GAME,
                    error = %format!("{error:#}"),
                    "Refinement skipped"
                );
                return None;
            }
        };
        if candidates.len() < 2 {
            // One match left is not the same as nothing to say. Battlefield 6
            // does this every session: the EA anti-cheat *launcher* matches the
            // install folder, wins the first identify because it starts first,
            // then exits into a service that does not match -- leaving bf6.exe
            // alone and the session named after a process that no longer
            // exists. Measured 2026-09-15.
            let named_is_alive = current
                .and_then(|signal| signal.process_id)
                .is_some_and(|pid| self.sensor.is_running(pid));
            let count = candidates.len();
            if let Some(survivor) = detect::lone_survivor(candidates, current, named_is_alive) {
                tracing::info!(
                    target: target::GAME,
                    pid = survivor.process_id,
                    matched_by = survivor.source,
                    "Game identified more precisely: {} (the only match left)",
                    survivor.name()
                );
                return Some(survivor);
            }
            tracing::debug!(
                target: target::GAME,
                candidates = count,
                "Refinement has nothing to arbitrate, keeping the current name"
            );
            return None;
        }

        let load = match self.sensor.rendering_load(self.config.detection.gpu_sample) {
            Ok(load) => load,
            Err(error) => {
                tracing::debug!(
                    target: target::GAME,
                    error = %format!("{error:#}"),
                    "Cannot read the GPU counters, keeping the first match"
                );
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

    fn fire_start(&self, signal: Option<&GameSignal>) {
        // A game Windows tracks but does not name is a session all the same;
        // the tray shows it as one, with its own wording.
        self.report(&Session::Playing(signal.cloned()));
        // Before the commands, so a crash between the two still leaves a
        // session to close.
        self.remember(signal);
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

    fn fire_stop(&self, signal: Option<&GameSignal>) -> Outcome {
        // Before the commands, not after: those can take fifteen seconds, and
        // an icon still showing a game that has ended for that long is the
        // thing anyone would notice.
        self.report(&Session::Idle);
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
        )
    }
}

#[cfg(test)]
mod tests;
