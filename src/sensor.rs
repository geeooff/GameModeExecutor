//! What the engine observes about the machine, behind one trait.
//!
//! The engine decides; it does not read the OS itself. Everything it needs to
//! know arrives through [`Sensor`]: whether the presence writer runs, when it
//! exits, which processes the Known Game List matches, whether a process is
//! still alive, and what the GPU is drawing. [`Windows`] answers from the real
//! machine. The engine's tests answer from a script, which is the only reason
//! the trait exists -- one implementation would not have earned one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::detect::known_games::KnownGames;
use crate::detect::presence_writer::{self, WaitOutcome};
use crate::detect::process::Snapshot;
use crate::detect::{GameSignal, gpu};
use crate::logging::target;
use crate::win::StopSignal;

pub trait Sensor {
    /// The presence writer's pid, when Windows has one running.
    fn writer_pid(&self) -> Option<u32>;

    /// Park until the writer exits, the stop is signalled, or `timeout`
    /// passes -- whichever comes first.
    fn wait_for_writer_exit(
        &self,
        pid: u32,
        stop: &StopSignal,
        timeout: Option<Duration>,
    ) -> Result<WaitOutcome>;

    /// Every running process the Known Game List matches, in the order the
    /// system lists them. An error means the list or the processes could not
    /// be read at all; no match is an empty `Vec`.
    fn candidates(&self) -> Result<Vec<GameSignal>>;

    /// Whether a process is still running.
    fn is_running(&self, pid: u32) -> bool;

    /// Share of the 3D GPU engine per process over `sample`, for the
    /// processes that have any.
    fn rendering_load(&self, sample: Duration) -> Result<HashMap<u32, f64>>;
}

/// The real machine.
pub struct Windows {
    /// Resolved from the registry once at startup, never hard-coded.
    writer_exe: PathBuf,
}

impl Windows {
    pub fn new() -> Result<Self> {
        let writer_exe = presence_writer::registered_exe()?;
        if !presence_writer::is_microsoft_default(&writer_exe) {
            tracing::warn!(
                target: target::WATCHER,
                writer = %writer_exe.display(),
                "The registered Game Bar presence writer is not the one Windows ships; \
                 detection follows whatever is registered"
            );
        }
        Ok(Self { writer_exe })
    }

    pub fn writer_exe(&self) -> &Path {
        &self.writer_exe
    }
}

impl Sensor for Windows {
    fn writer_pid(&self) -> Option<u32> {
        presence_writer::running_pid(&self.writer_exe)
    }

    fn wait_for_writer_exit(
        &self,
        pid: u32,
        stop: &StopSignal,
        timeout: Option<Duration>,
    ) -> Result<WaitOutcome> {
        presence_writer::wait_for_exit_until(pid, stop, timeout)
    }

    fn candidates(&self) -> Result<Vec<GameSignal>> {
        let known = KnownGames::load().context("cannot read Windows' known game list")?;
        let snapshot = Snapshot::take().context("cannot list running processes")?;
        Ok(known.candidates(&snapshot))
    }

    fn is_running(&self, pid: u32) -> bool {
        Snapshot::take()
            .ok()
            .is_some_and(|snapshot| snapshot.by_pid(pid).is_some())
    }

    fn rendering_load(&self, sample: Duration) -> Result<HashMap<u32, f64>> {
        gpu::rendering_load(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The real sensor against the real machine. Skipped on a hosted runner
    // for the same reason as the registration test it wraps.
    #[test]
    #[ignore = "reads the Game Bar registration, absent on Windows Server runners"]
    fn the_real_sensor_answers_every_question() {
        let sensor = Windows::new().expect("Game Bar is registered here");
        assert!(sensor.writer_exe().is_absolute());
        // Any answer is fine; the point is that none of them panics or fails
        // on a client machine.
        let _ = sensor.writer_pid();
        sensor
            .candidates()
            .expect("the known game list is readable");
        assert!(sensor.is_running(std::process::id()));
        assert!(!sensor.is_running(u32::MAX));
        let _ = sensor.rendering_load(Duration::from_millis(100));
    }
}
