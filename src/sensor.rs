//! What the engine observes about the machine, behind one trait.
//!
//! The engine decides; it does not read the OS itself. Everything it needs to
//! know arrives through [`Sensor`]: whether Windows says a game runs -- its
//! presence writer, or a process the person marked as a game by hand --
//! when that process exits, which processes the Known Game List matches,
//! whether a process is still alive, and what the GPU is drawing.
//! [`Windows`] answers from the real machine. The engine's tests answer from
//! a script, which is the only reason the trait exists -- one implementation
//! would not have earned one.
//!
//! The idle look is the one thing in the program that runs on a timer, so
//! [`Windows`] keeps it cheap: the process ids alone every look, a name only
//! for a process not seen before, a full snapshot every thirty seconds, and
//! the hand-made entries read again only when the list's key was written.
//! About 50 us a look instead of the 4 ms a snapshot costs, measured
//! 2026-09-23 in `docs/design/15-marked-games.md`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::detect::known_games::KnownGames;
use crate::detect::presence_writer::{self, WaitOutcome};
use crate::detect::process::{self, Snapshot, Tracker};
use crate::detect::{GameSignal, gpu, hand_made};
use crate::logging::target;
use crate::win::StopSignal;

/// What an idle look found running that makes a game session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sighting {
    /// Windows' presence writer: a title Windows knows.
    Writer(u32),
    /// A process the person marked as a game by hand, for which Windows
    /// never starts the writer. The signal names it exactly.
    HandMade(GameSignal),
}

impl Sighting {
    /// The process a session waits on.
    pub fn pid(&self) -> u32 {
        match self {
            Self::Writer(pid) => *pid,
            Self::HandMade(signal) => signal.process_id.unwrap_or_default(),
        }
    }
}

/// How a session found by a hand-made entry says where it came from.
pub const HAND_MADE: &str = "hand-made entry";

pub trait Sensor {
    /// What runs now that makes a session, if anything: the writer first,
    /// then a hand-made entry's process.
    fn sighting(&self) -> Option<Sighting>;

    /// Park until the process exits, the stop is signalled, or `timeout`
    /// passes -- whichever comes first.
    fn wait_for_exit(
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

/// A borrowed sensor answers as the sensor does: the supervisor keeps one
/// `Windows` for the life of the process and builds an engine on it for
/// each configuration.
impl<S: Sensor + ?Sized> Sensor for &S {
    fn sighting(&self) -> Option<Sighting> {
        (**self).sighting()
    }

    fn wait_for_exit(
        &self,
        pid: u32,
        stop: &StopSignal,
        timeout: Option<Duration>,
    ) -> Result<WaitOutcome> {
        (**self).wait_for_exit(pid, stop, timeout)
    }

    fn candidates(&self) -> Result<Vec<GameSignal>> {
        (**self).candidates()
    }

    fn is_running(&self, pid: u32) -> bool {
        (**self).is_running(pid)
    }

    fn rendering_load(&self, sample: Duration) -> Result<HashMap<u32, f64>> {
        (**self).rendering_load(sample)
    }
}

/// The real machine.
pub struct Windows {
    /// Resolved from the registry once at startup, never hard-coded.
    writer_exe: PathBuf,
    /// Its file name, lowercased, to find it among the running processes.
    writer_name: String,
    /// What the idle looks keep between them. One thread asks, so a cell
    /// is enough.
    look: RefCell<Look>,
}

/// What an idle look keeps between two looks.
#[derive(Default)]
struct Look {
    processes: Tracker,
    list: hand_made::Watch,
    hand_made: Vec<hand_made::Entry>,
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
        let writer_name = hand_made::file_name_of(&writer_exe.to_string_lossy()).to_lowercase();
        Ok(Self {
            writer_exe,
            writer_name,
            look: RefCell::new(Look::default()),
        })
    }

    pub fn writer_exe(&self) -> &Path {
        &self.writer_exe
    }
}

impl Look {
    /// The hand-made entries, read again when the list's key was written:
    /// a tick or an untick. A list that cannot be read keeps the last ones.
    fn refresh_list(&mut self) {
        if !self.list.changed() {
            return;
        }
        match hand_made::load() {
            Ok(entries) => {
                if entries != self.hand_made {
                    tracing::debug!(
                        target: target::GAME,
                        entries = entries.len(),
                        names = ?entries.iter().map(hand_made::Entry::display_name).collect::<Vec<_>>(),
                        "The games marked by hand in the Game Bar, as the list now has them"
                    );
                }
                self.hand_made = entries;
            }
            Err(error) => tracing::debug!(
                target: target::GAME,
                error = %format!("{error:#}"),
                "The games marked by hand cannot be read; the last ones read stay"
            ),
        }
    }
}

impl Sensor for Windows {
    fn sighting(&self) -> Option<Sighting> {
        let mut look = self.look.borrow_mut();
        look.refresh_list();
        if let Err(error) = look.processes.refresh(Instant::now()) {
            tracing::debug!(
                target: target::GAME,
                error = %format!("{error:#}"),
                "The running processes cannot be listed this time"
            );
            return None;
        }
        let writer = self.writer_exe.to_string_lossy().to_lowercase();
        for pid in look.processes.named(&self.writer_name) {
            // Same name elsewhere on disk is not the registered writer; a
            // path that cannot be read is taken as it, as before.
            match process::full_path(pid) {
                Some(path) if path.to_lowercase() != writer => continue,
                _ => return Some(Sighting::Writer(pid)),
            }
        }
        for entry in &look.hand_made {
            for pid in look.processes.named(entry.file_name_lower()) {
                if let Some(path) = process::full_path(pid)
                    && entry.is(&path)
                {
                    return Some(Sighting::HandMade(GameSignal {
                        source: HAND_MADE,
                        process_name: Some(entry.display_name().to_owned()),
                        process_id: Some(pid),
                        process_path: Some(path),
                    }));
                }
            }
        }
        None
    }

    fn wait_for_exit(
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
        let _ = sensor.sighting();
        let _ = sensor.sighting();
        sensor
            .candidates()
            .expect("the known game list is readable");
        assert!(sensor.is_running(std::process::id()));
        assert!(!sensor.is_running(u32::MAX));
        let _ = sensor.rendering_load(Duration::from_millis(100));
    }

    /// The whole chain on a real Windows, with no game: activating the
    /// presence writer's runtime class makes Windows start the writer, the
    /// engine sees a session, releasing the object ends it.
    ///
    /// Run by name, not by the checklist: an installed watcher on the same
    /// machine sees the same writer and runs the user's own commands.
    ///
    ///     cargo test -- --ignored a_real_activation_drives_a_session
    ///
    /// Worked on 2026-09-09 and 2026-09-17, did not on 2026-09-15 -- the
    /// activation resolved without a writer process, for a reason not
    /// understood. The test skips rather than fails in that case, and when a
    /// game is already running, since the writer is then not ours to release.
    #[test]
    #[ignore = "starts Windows' presence writer for real; run by name"]
    fn a_real_activation_drives_a_session() {
        use crate::engine::{Engine, Session};
        use std::sync::{Arc, Mutex};
        use windows::Win32::System::WinRT::{
            RO_INIT_MULTITHREADED, RoActivateInstance, RoInitialize,
        };
        use windows::core::HSTRING;

        // A sensor keeps the game list's key open, which stays on the thread
        // that opened it: this one looks first, the engine's own is built on
        // the engine's thread.
        let look = Windows::new().expect("Game Bar is registered here");
        if look.sighting().is_some() {
            eprintln!("skipped: a game is running, the writer is not ours to release");
            return;
        }
        drop(look);

        let mut config = crate::config::Config::default();
        config.detection.poll_interval = Duration::from_millis(50);
        config.detection.stop_delay = Duration::ZERO;
        config.detection.identify_after = Duration::ZERO;

        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = {
            let seen = Arc::clone(&seen);
            Arc::new(move |session: &Session| seen.lock().unwrap().push(session.clone()))
        };
        let stop = Arc::new(StopSignal::new().unwrap());
        let worker = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let sensor = Windows::new()?;
                Engine::new(config, sensor).reporting_to(sink).run(&stop)
            })
        };

        // SAFETY: initialises the Windows Runtime for this thread; no pointers.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.expect("RoInitialize");
        // SAFETY: the class id is a valid HSTRING that outlives the call.
        let object = unsafe { RoActivateInstance(&HSTRING::from(presence_writer::CLASS_ID)) };
        let started = std::time::Instant::now();
        let mut detected = false;
        while started.elapsed() < Duration::from_secs(5) {
            if seen
                .lock()
                .unwrap()
                .iter()
                .any(|s| matches!(s, Session::Playing(_)))
            {
                detected = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(object);
        if !detected {
            eprintln!("skipped: the activation started no presence writer this time");
            stop.signal();
            worker.join().unwrap().unwrap();
            return;
        }

        let released = std::time::Instant::now();
        while released.elapsed() < Duration::from_secs(10) {
            if seen.lock().unwrap().last() == Some(&Session::Idle) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        stop.signal();
        worker.join().unwrap().unwrap();

        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2, "one session, both edges: {seen:?}");
        assert!(matches!(seen[0], Session::Playing(_)), "{seen:?}");
        assert_eq!(seen[1], Session::Idle);
    }
}
