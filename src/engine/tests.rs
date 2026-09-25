//! The engine, driven through a scripted sensor.
//!
//! Every test here runs a whole `Engine::run`: the sensor answers from a
//! script, the commands are real `cmd.exe` processes, the marker is a real
//! file in a scratch folder, and the stop signal is a real Win32 event. What
//! is scripted is only what Windows would have said.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::config::{Action, Event, Mode};

// ------------------------------------------------------------ the sensor --

/// A machine that says what the test told it to, in order.
struct Scripted {
    /// Answers to `sighting`, one per call. When they run out the stop is
    /// signalled, which is how `run` is made to return.
    writer: RefCell<VecDeque<Option<Sighting>>>,
    /// Answers to `wait_for_exit`, one per call.
    waits: RefCell<VecDeque<WaitOutcome>>,
    /// Answers to `candidates`, one per call; the last one repeats.
    candidates: RefCell<VecDeque<Vec<GameSignal>>>,
    alive: HashSet<u32>,
    /// Answers to `rendering_load`, one per call; the last one repeats.
    loads: RefCell<VecDeque<HashMap<u32, f64>>>,
    /// The timeout each `wait_for_exit` was asked for, in order.
    timeouts: RefCell<Vec<Option<Duration>>>,
    /// `candidates` fails, as it does when the list cannot be read.
    list_unreadable: bool,
    /// `rendering_load` fails, as it does on an account that may not read
    /// the counters.
    counters_unreadable: bool,
    /// The session ends as the writer does: the stop is signalled in the
    /// same instant `wait_for_exit` reports the exit, as a logoff
    /// does when Windows kills the writer before the watcher is told.
    session_ends_with_writer: bool,
    /// A stop reported by `wait_for_exit` is a handover, as
    /// `stop --handover` from an update makes it.
    stops_by_handover: bool,
    /// A stop reported by `wait_for_exit` is a reload, as a change
    /// to the configuration file makes it.
    stops_by_reload: bool,
    stop: Arc<StopSignal>,
}

impl Scripted {
    fn new(stop: &Arc<StopSignal>) -> Self {
        Self {
            writer: RefCell::new(VecDeque::new()),
            waits: RefCell::new(VecDeque::new()),
            candidates: RefCell::new(VecDeque::new()),
            alive: HashSet::new(),
            loads: RefCell::new(VecDeque::new()),
            timeouts: RefCell::new(Vec::new()),
            list_unreadable: false,
            counters_unreadable: false,
            session_ends_with_writer: false,
            stops_by_handover: false,
            stops_by_reload: false,
            stop: Arc::clone(stop),
        }
    }

    fn stops_by_handover(mut self) -> Self {
        self.stops_by_handover = true;
        self
    }

    fn stops_by_reload(mut self) -> Self {
        self.stops_by_reload = true;
        self
    }

    fn list_unreadable(mut self) -> Self {
        self.list_unreadable = true;
        self
    }

    fn counters_unreadable(mut self) -> Self {
        self.counters_unreadable = true;
        self
    }

    fn session_ends_with_writer(mut self) -> Self {
        self.session_ends_with_writer = true;
        self
    }

    /// Answers to `sighting` that are the presence writer, or nothing.
    fn writer(self, answers: &[Option<u32>]) -> Self {
        self.writer
            .borrow_mut()
            .extend(answers.iter().map(|answer| answer.map(Sighting::Writer)));
        self
    }

    /// Answers to `sighting` of any kind.
    fn sightings(self, answers: &[Option<Sighting>]) -> Self {
        self.writer.borrow_mut().extend(answers.iter().cloned());
        self
    }

    fn waits(self, answers: &[WaitOutcome]) -> Self {
        self.waits.borrow_mut().extend(answers.iter().cloned());
        self
    }

    fn candidates(self, answers: &[&[GameSignal]]) -> Self {
        self.candidates
            .borrow_mut()
            .extend(answers.iter().map(|list| list.to_vec()));
        self
    }

    fn alive(mut self, pids: &[u32]) -> Self {
        self.alive.extend(pids.iter().copied());
        self
    }

    /// One answer to `rendering_load`; call again for the next attempt's.
    fn rendering(self, load: &[(u32, f64)]) -> Self {
        self.loads
            .borrow_mut()
            .push_back(load.iter().copied().collect());
        self
    }
}

impl Sensor for Scripted {
    fn sighting(&self) -> Option<Sighting> {
        match self.writer.borrow_mut().pop_front() {
            Some(answer) => answer,
            None => {
                self.stop.signal();
                None
            }
        }
    }

    fn wait_for_exit(
        &self,
        _pid: u32,
        _stop: &StopSignal,
        timeout: Option<Duration>,
    ) -> Result<WaitOutcome> {
        self.timeouts.borrow_mut().push(timeout);
        let outcome = self
            .waits
            .borrow_mut()
            .pop_front()
            .expect("the script ran out of waits");
        if self.session_ends_with_writer && outcome == WaitOutcome::Exited {
            self.stop.signal();
        }
        if self.stops_by_handover && outcome == WaitOutcome::Stopped {
            self.stop.signal_handover();
        }
        if self.stops_by_reload && outcome == WaitOutcome::Stopped {
            self.stop.signal_reload();
        }
        Ok(outcome)
    }

    fn candidates(&self) -> Result<Vec<GameSignal>> {
        if self.list_unreadable {
            anyhow::bail!("scripted: the known game list is unreadable");
        }
        let mut answers = self.candidates.borrow_mut();
        if answers.len() > 1 {
            return Ok(answers.pop_front().unwrap());
        }
        Ok(answers.front().cloned().unwrap_or_default())
    }

    fn is_running(&self, pid: u32) -> bool {
        self.alive.contains(&pid)
    }

    fn rendering_load(&self, _sample: Duration) -> Result<HashMap<u32, f64>> {
        if self.counters_unreadable {
            anyhow::bail!("scripted: the counters are unreadable");
        }
        let mut answers = self.loads.borrow_mut();
        if answers.len() > 1 {
            return Ok(answers.pop_front().unwrap());
        }
        Ok(answers.front().cloned().unwrap_or_default())
    }
}

// ----------------------------------------------------------- the fixtures --

fn game(pid: u32, name: &str) -> GameSignal {
    GameSignal {
        source: "test",
        process_name: Some(name.to_owned()),
        process_id: Some(pid),
        process_path: None,
    }
}

/// A configuration that runs nothing and waits for nothing, so a scripted
/// session takes milliseconds.
fn quick_config() -> Config {
    let mut config = Config::default();
    config.detection.poll_interval = Duration::from_millis(1);
    config.detection.stop_delay = Duration::ZERO;
    config.detection.identify_after = Duration::from_millis(1);
    config.detection.gpu_sample = Duration::ZERO;
    config
}

/// `cmd.exe /c exit N`, waited for, so the outcome carries a verdict.
fn exit_with(code: u8) -> Action {
    Action {
        name: Some(format!("exit {code}")),
        program: "cmd.exe".into(),
        args: vec!["/c".to_owned(), format!("exit {code}")],
        wait: true,
        timeout: Some(Duration::from_secs(10)),
        ..Action::default()
    }
}

/// A command whose only effect is a file, so a test can see whether it ran.
///
/// The file name is relative and the folder goes in `working_dir`: std quotes
/// an argument with spaces or quotes in a way `cmd.exe` does not read back,
/// so the redirection has to stay free of both.
fn touch(path: &Path) -> Action {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    Action {
        name: Some("touch".to_owned()),
        program: "cmd.exe".into(),
        args: vec!["/c".to_owned(), format!("type nul > {name}")],
        working_dir: Some(path.parent().unwrap().to_path_buf()),
        wait: true,
        timeout: Some(Duration::from_secs(10)),
        ..Action::default()
    }
}

/// A command that appends its event and the game's name to `path`, so a
/// test can read which edges ran, in order, and under which name.
///
/// One argument per word, for the reason `touch` gives.
fn log_edge(path: &Path) -> Action {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    Action {
        name: Some("log edge".to_owned()),
        program: "cmd.exe".into(),
        args: ["/c", "echo", "{event}", "{process_name}", ">>", &name]
            .map(str::to_owned)
            .to_vec(),
        working_dir: Some(path.parent().unwrap().to_path_buf()),
        wait: true,
        timeout: Some(Duration::from_secs(10)),
        ..Action::default()
    }
}

/// The edges `log_edge` wrote, one `event name` per line.
fn edges(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| line.trim().to_owned())
        .collect()
}

fn stop_event(actions: Vec<Action>) -> Event {
    Event {
        mode: Mode::Series,
        actions,
    }
}

fn scratch() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "gamemode-executor-engine-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Records every session change the engine reports.
fn recorder() -> (SessionSink, Arc<Mutex<Vec<Session>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let seen = Arc::clone(&seen);
        Arc::new(move |session: &Session| seen.lock().unwrap().push(session.clone()))
    };
    (sink, seen)
}

fn seen(log: &Arc<Mutex<Vec<Session>>>) -> Vec<Session> {
    log.lock().unwrap().clone()
}

// --------------------------------------------------------------- sessions --

#[test]
fn a_session_is_reported_on_both_edges_and_leaves_no_marker() {
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Exited])
        .candidates(&[&[game(10, "game.exe")]]);
    let (sink, log) = recorder();
    let marker = Marker::in_dir(&scratch());
    let marker_path = marker.path().to_path_buf();

    let mut engine = Engine::new(quick_config(), sensor)
        .reporting_to(sink)
        .remembering(marker);
    engine.run(&stop).unwrap();

    assert_eq!(
        seen(&log),
        vec![Session::Playing(Some(game(10, "game.exe"))), Session::Idle]
    );
    assert!(
        !marker_path.exists(),
        "a game that stopped on its own leaves no marker"
    );
}

#[test]
fn a_game_windows_does_not_name_is_still_a_session() {
    // The tray used to show "no game detected" here: the sink was handed
    // `None` for "unnamed", which was also its word for "nothing". This test
    // is the one that would have caught it.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Exited])
        .candidates(&[&[]]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(seen(&log), vec![Session::Playing(None), Session::Idle]);
}

/// A process the person marked as a game by hand, as the sensor reports it.
fn marked(pid: u32, name: &str) -> GameSignal {
    GameSignal {
        source: HAND_MADE,
        process_name: Some(name.to_owned()),
        process_id: Some(pid),
        process_path: Some(format!(r"C:\Games\{name}")),
    }
}

/// What the sensor reports for it.
fn hand_made(pid: u32, name: &str) -> Sighting {
    Sighting::HandMade {
        pid,
        game: marked(pid, name),
    }
}

#[test]
fn a_game_marked_by_hand_is_a_session_from_its_launch() {
    // Windows never starts the writer for a title the person marked by
    // hand; its entry's process running is the session. The commands run on
    // both edges, and the name is the entry's: the GPU is not asked, so a
    // candidate drawing more cannot rename it.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .sightings(&[None, Some(hand_made(30, "TOS.exe"))])
        .waits(&[WaitOutcome::Exited])
        .candidates(&[&[game(31, "other.exe")]])
        .rendering(&[(31, 90.0)]);
    let dir = scratch();
    let started = dir.join("start-ran");
    let stopped = dir.join("stop-ran");
    let mut config = quick_config();
    config.on_game_start = stop_event(vec![touch(&started)]);
    config.on_game_stop = stop_event(vec![touch(&stopped)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(config, sensor)
        .reporting_to(sink)
        .remembering(Marker::in_dir(&dir));
    engine.run(&stop).unwrap();

    assert!(started.exists() && stopped.exists(), "both edges ran");
    assert_eq!(
        seen(&log),
        vec![Session::Playing(Some(marked(30, "TOS.exe"))), Session::Idle],
        "named by its entry, and never renamed"
    );
    assert!(Marker::in_dir(&dir).pending().is_none());
}

#[test]
fn a_session_marked_by_hand_is_resumed_after_a_handover() {
    // The same resume as for the writer: the marker open, the game marked by
    // hand still running, nothing run until it ends.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .sightings(&[Some(hand_made(30, "TOS.exe"))])
        .waits(&[WaitOutcome::Exited]);
    let dir = scratch();
    let marker = Marker::in_dir(&dir);
    marker.open(Some("TOS.exe"), "earlier").unwrap();
    let started = dir.join("start-ran");
    let stopped = dir.join("stop-ran");
    let mut config = quick_config();
    config.on_game_start = stop_event(vec![touch(&started)]);
    config.on_game_stop = stop_event(vec![touch(&stopped)]);

    let mut engine = Engine::new(config, sensor).remembering(marker);
    engine.run(&stop).unwrap();

    assert!(!started.exists(), "the start commands did not run again");
    assert!(
        stopped.exists(),
        "the stop commands ran when the game ended"
    );
    assert!(Marker::in_dir(&dir).pending().is_none());
}

#[test]
fn a_game_relaunched_within_the_grace_keeps_the_session() {
    // The game marked by hand quits and is running again before
    // `stop_delay` is out -- a launcher restarting it: one session.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .sightings(&[
            Some(hand_made(30, "TOS.exe")),
            Some(hand_made(32, "TOS.exe")),
        ])
        .waits(&[WaitOutcome::Exited, WaitOutcome::Exited]);
    let (sink, log) = recorder();
    let mut config = quick_config();
    config.detection.stop_delay = Duration::from_millis(50);

    // Borrowed, so the script can be asked afterwards what was used.
    let mut engine = Engine::new(config, &sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert!(
        sensor.waits.borrow().is_empty(),
        "the relaunched game was waited on too"
    );
    assert_eq!(
        seen(&log),
        vec![Session::Playing(Some(marked(30, "TOS.exe"))), Session::Idle],
        "one start, one end"
    );
}

#[test]
fn the_marker_is_written_when_the_game_starts_and_names_it() {
    let stop = Arc::new(StopSignal::new().unwrap());
    // The session is stopped from outside, so the marker is still there to
    // read: a normal stop would have removed it.
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Stopped])
        .candidates(&[&[game(10, "game.exe")]]);
    let marker = Marker::in_dir(&scratch());
    let path = marker.path().to_path_buf();
    let mut config = quick_config();
    // Nothing to confirm, so the marker stays.
    config.general.stop_actions_on_exit = true;

    let mut engine = Engine::new(config, sensor).remembering(marker);
    engine.run(&stop).unwrap();

    let pending = Marker::in_dir(path.parent().unwrap()).pending().unwrap();
    assert_eq!(pending.game.as_deref(), Some("game.exe"));
}

// --------------------------------------------------------------- naming --

#[test]
fn the_launcher_that_died_is_replaced_by_the_one_match_left() {
    // Battlefield 6's shape: the anti-cheat launcher matches first, then
    // exits, leaving the game alone.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::Exited])
        .candidates(&[&[game(5, "launcher.exe")], &[game(10, "bf6.exe")]])
        .alive(&[10]);
    let (sink, log) = recorder();
    let marker = Marker::in_dir(&scratch());
    let marker_dir = marker.path().parent().unwrap().to_path_buf();

    let mut engine = Engine::new(quick_config(), sensor)
        .reporting_to(sink)
        .remembering(marker);
    engine.run(&stop).unwrap();

    assert_eq!(
        seen(&log),
        vec![
            Session::Playing(Some(game(5, "launcher.exe"))),
            Session::Playing(Some(game(10, "bf6.exe"))),
            Session::Idle,
        ]
    );
    // The marker followed the rename, then went with the normal stop.
    assert!(Marker::in_dir(&marker_dir).pending().is_none());
}

#[test]
fn a_living_name_is_kept_when_the_only_candidate_is_another() {
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::Exited])
        .candidates(&[&[game(5, "first.exe")], &[game(10, "other.exe")]])
        .alive(&[5, 10]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(
        seen(&log),
        vec![Session::Playing(Some(game(5, "first.exe"))), Session::Idle]
    );
}

#[test]
fn the_gpu_hands_the_session_to_the_process_that_is_drawing() {
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::Exited])
        .candidates(&[&[game(5, "anticheat.exe"), game(10, "bf6.exe")]])
        .alive(&[5, 10])
        .rendering(&[(5, 0.0), (10, 75.0)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(
        seen(&log),
        vec![
            Session::Playing(Some(game(5, "anticheat.exe"))),
            Session::Playing(Some(game(10, "bf6.exe"))),
            Session::Idle,
        ]
    );
}

/// The timeouts a session asks for when the refinement makes `attempts`
/// attempts and then waits for the end with none.
fn asked(attempts: u32) -> Vec<Option<Duration>> {
    let mut timeouts = vec![Some(quick_config().detection.identify_after); attempts as usize];
    timeouts.push(None);
    timeouts
}

#[test]
fn nothing_rendering_yet_is_asked_again_until_something_is() {
    // Battlefield 6 on 2026-09-16: every candidate at 0.0 % ten seconds
    // before the attempt, bf6.exe at 75 % ten seconds after. A loading
    // screen longer than the first interval used to keep the first name for
    // the whole session; the attempt that reads nothing now does not count.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[
            WaitOutcome::TimedOut,
            WaitOutcome::TimedOut,
            WaitOutcome::Exited,
        ])
        .candidates(&[&[game(5, "anticheat.exe"), game(10, "bf6.exe")]])
        .alive(&[5, 10])
        .rendering(&[(5, 0.0), (10, 0.0)])
        .rendering(&[(5, 0.0), (10, 75.0)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), &sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(
        seen(&log),
        vec![
            Session::Playing(Some(game(5, "anticheat.exe"))),
            Session::Playing(Some(game(10, "bf6.exe"))),
            Session::Idle,
        ]
    );
    assert_eq!(
        *sensor.timeouts.borrow(),
        asked(2),
        "the rename was the verdict: no third attempt"
    );
}

#[test]
fn a_session_with_no_verdict_stops_asking_after_the_last_attempt() {
    // Nothing ever renders: the attempts run out and the first name stays,
    // and the wait for the end asks for no timeout any more.
    let stop = Arc::new(StopSignal::new().unwrap());
    let mut waits = vec![WaitOutcome::TimedOut; REFINE_ATTEMPTS as usize];
    waits.push(WaitOutcome::Exited);
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&waits)
        .candidates(&[&[game(5, "anticheat.exe"), game(10, "bf6.exe")]])
        .alive(&[5, 10])
        .rendering(&[(5, 0.0), (10, 0.0)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), &sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(
        seen(&log),
        vec![
            Session::Playing(Some(game(5, "anticheat.exe"))),
            Session::Idle
        ]
    );
    assert_eq!(*sensor.timeouts.borrow(), asked(REFINE_ATTEMPTS));
}

#[test]
fn the_launcher_that_outlives_the_first_attempt_is_replaced_on_a_later_one() {
    // The launcher still there and nothing rendering at the first attempt;
    // by the second the launcher has gone and the game is the one match
    // left. The survivor rule needs no GPU, and it gets its chance.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[
            WaitOutcome::TimedOut,
            WaitOutcome::TimedOut,
            WaitOutcome::Exited,
        ])
        .candidates(&[
            &[game(5, "launcher.exe")],
            &[game(5, "launcher.exe"), game(10, "bf6.exe")],
            &[game(10, "bf6.exe")],
        ])
        .alive(&[10])
        .rendering(&[(5, 0.0), (10, 0.0)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), &sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(
        seen(&log),
        vec![
            Session::Playing(Some(game(5, "launcher.exe"))),
            Session::Playing(Some(game(10, "bf6.exe"))),
            Session::Idle,
        ]
    );
    assert_eq!(*sensor.timeouts.borrow(), asked(2));
}

#[test]
fn a_game_the_list_did_not_match_at_first_is_named_when_it_does() {
    // No match is not a verdict either: the session started unnamed, and a
    // later attempt that finds one match takes it.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[
            WaitOutcome::TimedOut,
            WaitOutcome::TimedOut,
            WaitOutcome::Exited,
        ])
        .candidates(&[&[], &[], &[game(10, "game.exe")]]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), &sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(
        seen(&log),
        vec![
            Session::Playing(None),
            Session::Playing(Some(game(10, "game.exe"))),
            Session::Idle,
        ]
    );
    assert_eq!(*sensor.timeouts.borrow(), asked(2));
}

#[test]
fn the_one_match_being_the_name_in_use_is_a_verdict() {
    // Starfield and Skyrim, most sessions: one process matches and it is
    // the one named. Nothing to arbitrate, and nothing to ask again.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::Exited])
        .candidates(&[&[game(10, "Starfield.exe")]])
        .alive(&[10]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), &sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(seen(&log).len(), 2);
    assert_eq!(*sensor.timeouts.borrow(), asked(1));
}

#[test]
fn a_confirmed_name_is_not_reported_again() {
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::Exited])
        .candidates(&[&[game(10, "bf6.exe"), game(5, "anticheat.exe")]])
        .alive(&[5, 10])
        .rendering(&[(10, 90.0)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), &sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    // Two changes only: the start and the stop. The confirmation is a log
    // line, not a report, or the tray would redraw the same icon.
    assert_eq!(seen(&log).len(), 2);
    assert_eq!(
        *sensor.timeouts.borrow(),
        asked(1),
        "and a verdict: nothing to ask again"
    );
}

#[test]
fn refinement_is_skipped_when_identify_after_is_zero() {
    let stop = Arc::new(StopSignal::new().unwrap());
    // No TimedOut in the script: with identify_after = 0 the engine never
    // asks for one, so a script that offered it would go unconsumed.
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Exited])
        .candidates(&[&[game(5, "first.exe")]]);
    let (sink, log) = recorder();
    let mut config = quick_config();
    config.detection.identify_after = Duration::ZERO;

    let mut engine = Engine::new(config, &sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(seen(&log).len(), 2);
    assert_eq!(*sensor.timeouts.borrow(), vec![None]);
}

// ------------------------------------------------- the writer blinking --

#[test]
fn a_writer_that_comes_back_within_the_grace_keeps_the_session_open() {
    let stop = Arc::new(StopSignal::new().unwrap());
    // Writer 7 exits, 8 appears while the grace runs, then 8 exits for good.
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7), Some(8)])
        .waits(&[WaitOutcome::Exited, WaitOutcome::Exited])
        .candidates(&[&[game(10, "game.exe")]]);
    let (sink, log) = recorder();
    let mut config = quick_config();
    config.detection.stop_delay = Duration::from_millis(200);

    let mut engine = Engine::new(config, sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    // One session, not two: the blink was absorbed. (The script runs out
    // during the second grace, which signals the stop, so the session ends
    // through the mid-game branch; both branches report the same edge.)
    assert_eq!(
        seen(&log),
        vec![Session::Playing(Some(game(10, "game.exe"))), Session::Idle]
    );
}

// -------------------------------------------------- stopping mid-game --

#[test]
fn a_stop_mid_game_runs_the_stop_commands_and_closes_the_marker_when_confirmed() {
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Stopped])
        .candidates(&[&[game(10, "game.exe")]]);
    let dir = scratch();
    let ran = dir.join("stop-ran");
    let mut config = quick_config();
    config.general.stop_actions_on_exit = true;
    config.on_game_stop = stop_event(vec![touch(&ran)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(config, sensor)
        .reporting_to(sink)
        .remembering(Marker::in_dir(&dir));
    engine.run(&stop).unwrap();

    assert!(ran.exists(), "the stop commands ran");
    assert!(
        Marker::in_dir(&dir).pending().is_none(),
        "confirmed, so closed"
    );
    assert_eq!(seen(&log).last(), Some(&Session::Idle));
}

#[test]
fn a_stop_mid_game_keeps_the_marker_when_a_command_fails() {
    // The logoff shape: the command is started and does not succeed. The
    // marker stays so the next start runs the commands again.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Stopped])
        .candidates(&[&[game(10, "game.exe")]]);
    let dir = scratch();
    let mut config = quick_config();
    config.general.stop_actions_on_exit = true;
    config.on_game_stop = stop_event(vec![exit_with(1)]);

    let mut engine = Engine::new(config, sensor).remembering(Marker::in_dir(&dir));
    engine.run(&stop).unwrap();

    let pending = Marker::in_dir(&dir).pending().expect("marker kept");
    assert_eq!(pending.game.as_deref(), Some("game.exe"));
}

#[test]
fn a_stop_mid_game_with_only_fire_and_forget_commands_keeps_the_marker() {
    // Nothing waited for is nothing confirmed, deliberately: the failure this
    // exists for is a process created and dying unseen.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Stopped])
        .candidates(&[&[game(10, "game.exe")]]);
    let dir = scratch();
    let mut config = quick_config();
    config.general.stop_actions_on_exit = true;
    let mut fire_and_forget = exit_with(0);
    fire_and_forget.wait = false;
    config.on_game_stop = stop_event(vec![fire_and_forget]);

    let mut engine = Engine::new(config, sensor).remembering(Marker::in_dir(&dir));
    engine.run(&stop).unwrap();

    assert!(Marker::in_dir(&dir).pending().is_some());
}

#[test]
fn a_writer_killed_by_the_session_ending_is_a_stop_mid_game() {
    // Measured at logoff on 2026-09-17: Windows killed the writer 5 ms after
    // asking the session to end, the wait reported the exit rather than the
    // stop, and the ordinary path removed the marker after a command that
    // had died unborn. A stop that is set by the time the writer is gone is
    // the mid-game case, whichever of the two arrived first.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Exited])
        .session_ends_with_writer()
        .candidates(&[&[game(10, "game.exe")]]);
    let dir = scratch();
    let mut config = quick_config();
    // A grace period, so the stop is also seen through it.
    config.detection.stop_delay = Duration::from_millis(200);
    config.general.stop_actions_on_exit = true;
    config.on_game_stop = stop_event(vec![exit_with(1)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(config, sensor)
        .reporting_to(sink)
        .remembering(Marker::in_dir(&dir));
    engine.run(&stop).unwrap();

    assert_eq!(seen(&log).last(), Some(&Session::Idle));
    let pending = Marker::in_dir(&dir)
        .pending()
        .expect("kept for the next start");
    assert_eq!(pending.game.as_deref(), Some("game.exe"));
}

#[test]
fn opting_out_of_stop_on_exit_runs_nothing_and_closes_the_marker() {
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Stopped])
        .candidates(&[&[game(10, "game.exe")]]);
    let dir = scratch();
    let ran = dir.join("stop-ran");
    let mut config = quick_config();
    config.general.stop_actions_on_exit = false;
    config.on_game_stop = stop_event(vec![touch(&ran)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(config, sensor)
        .reporting_to(sink)
        .remembering(Marker::in_dir(&dir));
    engine.run(&stop).unwrap();

    assert!(!ran.exists(), "opted out: the stop commands did not run");
    assert!(
        Marker::in_dir(&dir).pending().is_none(),
        "and the opt-out is not undone at the next start"
    );
    // The tray was not told the session ended: nothing changed for it.
    assert_eq!(
        seen(&log),
        vec![Session::Playing(Some(game(10, "game.exe")))]
    );
}

// ------------------------------------------------- two games in a row --

#[test]
fn two_games_one_after_the_other_are_two_sessions_each_named() {
    // Measured 2026-09-25 with American Truck Simulator and Euro Truck
    // Simulator 2: Windows released the writer within a second of the
    // first game's exit and started another for the second, twelve seconds
    // later. Two sessions, each identified and refined afresh, each edge
    // under its own name.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7), None, Some(8)])
        .waits(&[
            WaitOutcome::TimedOut,
            WaitOutcome::Exited,
            WaitOutcome::TimedOut,
            WaitOutcome::Exited,
        ])
        .candidates(&[
            &[game(10, "amtrucks.exe")],
            &[game(10, "amtrucks.exe")],
            &[game(20, "eurotrucks2.exe")],
        ])
        .alive(&[10, 20]);
    let dir = scratch();
    let log_file = dir.join("edges.txt");
    let mut config = quick_config();
    config.on_game_start = stop_event(vec![log_edge(&log_file)]);
    config.on_game_stop = stop_event(vec![log_edge(&log_file)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(config, &sensor)
        .reporting_to(sink)
        .remembering(Marker::in_dir(&dir));
    engine.run(&stop).unwrap();

    assert_eq!(
        edges(&log_file),
        [
            "game_start amtrucks.exe",
            "game_stop amtrucks.exe",
            "game_start eurotrucks2.exe",
            "game_stop eurotrucks2.exe",
        ]
    );
    assert_eq!(
        seen(&log),
        vec![
            Session::Playing(Some(game(10, "amtrucks.exe"))),
            Session::Idle,
            Session::Playing(Some(game(20, "eurotrucks2.exe"))),
            Session::Idle,
        ]
    );
    let mut asked_twice = asked(1);
    asked_twice.extend(asked(1));
    assert_eq!(
        *sensor.timeouts.borrow(),
        asked_twice,
        "the second session had its own refinement"
    );
    assert!(Marker::in_dir(&dir).pending().is_none());
}

#[test]
fn two_games_under_one_writer_are_one_session_named_after_the_first() {
    // Measured 2026-09-25: Euro Truck Simulator 2 started while American
    // Truck Simulator ran, and Windows kept the one writer for both, until
    // after the second had exited; the same with Wreckfest 2 from Steam and
    // Starfield from the Store. One session: the commands right, once
    // on each edge, but the name the first game's throughout -- the
    // refinement had settled before the second game started. Naming it
    // would take the wait on the named process, left with Lot 18.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::Exited])
        .candidates(&[
            &[game(10, "amtrucks.exe")],
            &[game(10, "amtrucks.exe")],
            &[game(10, "amtrucks.exe"), game(20, "eurotrucks2.exe")],
        ])
        .alive(&[10, 20]);
    let dir = scratch();
    let log_file = dir.join("edges.txt");
    let mut config = quick_config();
    config.on_game_start = stop_event(vec![log_edge(&log_file)]);
    config.on_game_stop = stop_event(vec![log_edge(&log_file)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(config, &sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(
        edges(&log_file),
        ["game_start amtrucks.exe", "game_stop amtrucks.exe"]
    );
    assert_eq!(
        seen(&log),
        vec![
            Session::Playing(Some(game(10, "amtrucks.exe"))),
            Session::Idle
        ]
    );
}

// ------------------------------------------------------------- handover --

#[test]
fn a_handover_mid_game_runs_nothing_and_leaves_the_session_open() {
    // An update stops the watcher while a game is on. The stop commands must
    // not run -- the next watcher resumes the session within the second --
    // and the marker must still say the session is open.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Stopped])
        .stops_by_handover()
        .candidates(&[&[game(10, "game.exe")]]);
    let dir = scratch();
    let ran = dir.join("stop-ran");
    let mut config = quick_config();
    config.general.stop_actions_on_exit = true;
    config.on_game_stop = stop_event(vec![touch(&ran)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(config, sensor)
        .reporting_to(sink)
        .remembering(Marker::in_dir(&dir));
    engine.run(&stop).unwrap();

    assert!(!ran.exists(), "the stop commands did not run");
    let pending = Marker::in_dir(&dir)
        .pending()
        .expect("the session stays open");
    assert_eq!(pending.game.as_deref(), Some("game.exe"));
    assert_eq!(
        seen(&log),
        vec![Session::Playing(Some(game(10, "game.exe")))],
        "the session was never reported as ended"
    );
}

#[test]
fn a_reload_mid_game_is_a_handover_to_the_next_engine() {
    // The configuration changed while a game is on. The engine stops as for
    // an update -- nothing runs, the session stays open -- and the signal
    // says so, so the supervisor knows to build the next engine rather than
    // return; taking the reload clears it for that engine's run.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Stopped])
        .stops_by_reload()
        .candidates(&[&[game(10, "game.exe")]]);
    let dir = scratch();
    let ran = dir.join("stop-ran");
    let mut config = quick_config();
    config.on_game_stop = stop_event(vec![touch(&ran)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(config, sensor)
        .reporting_to(sink)
        .remembering(Marker::in_dir(&dir));
    engine.run(&stop).unwrap();

    assert!(!ran.exists(), "the stop commands did not run");
    assert!(
        Marker::in_dir(&dir).pending().is_some(),
        "the session stays open"
    );
    assert_eq!(
        seen(&log),
        vec![Session::Playing(Some(game(10, "game.exe")))],
        "the session was never reported as ended"
    );
    assert_eq!(stop.reason(), StopReason::Reload);
    assert!(stop.take_reload(), "the reload is the supervisor's to take");
    assert!(!stop.is_set(), "and the next engine waits afresh");
}

#[test]
fn a_session_handed_over_is_resumed_without_running_anything() {
    // The next watcher starts with the marker open and the writer alive: it
    // takes the session up -- name from the marker, icon active -- and runs
    // neither the start commands, which already ran, nor the stop commands,
    // which are for when the game ends. Then the game ends, and they run.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::Exited]);
    let dir = scratch();
    let marker = Marker::in_dir(&dir);
    marker.open(Some("game.exe"), "earlier").unwrap();
    let started = dir.join("start-ran");
    let stopped = dir.join("stop-ran");
    let mut config = quick_config();
    config.on_game_start = stop_event(vec![touch(&started)]);
    config.on_game_stop = stop_event(vec![touch(&stopped)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(config, sensor)
        .reporting_to(sink)
        .remembering(marker);
    engine.run(&stop).unwrap();

    assert!(!started.exists(), "the start commands did not run again");
    assert!(
        stopped.exists(),
        "the stop commands ran when the game ended"
    );
    assert!(
        Marker::in_dir(&dir).pending().is_none(),
        "and the session closed as usual"
    );
    let resumed = GameSignal {
        source: "resumed",
        process_name: Some("game.exe".to_owned()),
        process_id: None,
        process_path: None,
    };
    assert_eq!(
        seen(&log),
        vec![Session::Playing(Some(resumed)), Session::Idle],
        "resumed as playing, then ended"
    );
}

#[test]
fn a_session_handed_over_whose_game_ended_meanwhile_is_closed_at_start() {
    // Same marker, but the writer is gone by the time the next watcher
    // starts: the ordinary recovery, the stop commands run before watching.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop).writer(&[None]);
    let dir = scratch();
    let marker = Marker::in_dir(&dir);
    marker.open(Some("game.exe"), "earlier").unwrap();
    let stopped = dir.join("stop-ran");
    let mut config = quick_config();
    config.on_game_stop = stop_event(vec![touch(&stopped)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(config, sensor)
        .reporting_to(sink)
        .remembering(marker);
    engine.run(&stop).unwrap();

    assert!(stopped.exists(), "the stop commands ran at start");
    assert!(Marker::in_dir(&dir).pending().is_none());
    assert_eq!(
        seen(&log),
        vec![Session::Idle],
        "the session left open is reported closed, and that is all"
    );
}

// ------------------------------------------------------------- recovery --

#[test]
fn a_marker_left_behind_runs_the_stop_commands_before_watching() {
    let stop = Arc::new(StopSignal::new().unwrap());
    // No writer at all: the engine should recover, then find nothing and stop.
    let sensor = Scripted::new(&stop);
    let dir = scratch();
    let marker = Marker::in_dir(&dir);
    marker.open(Some("Starfield.exe"), "earlier").unwrap();
    let ran = dir.join("stop-ran");
    let mut config = quick_config();
    config.on_game_stop = stop_event(vec![touch(&ran)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(config, sensor)
        .reporting_to(sink)
        .remembering(marker);
    engine.run(&stop).unwrap();

    assert!(ran.exists(), "the stop commands ran at start");
    assert!(
        Marker::in_dir(&dir).pending().is_none(),
        "and the marker is gone"
    );
    assert_eq!(
        seen(&log),
        vec![Session::Idle],
        "the session left open is reported closed; recovery is not a session"
    );
}

#[test]
fn without_a_marker_nothing_runs_at_start() {
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop);
    let dir = scratch();
    let ran = dir.join("stop-ran");
    let mut config = quick_config();
    config.on_game_stop = stop_event(vec![touch(&ran)]);

    let mut engine = Engine::new(config, sensor).remembering(Marker::in_dir(&dir));
    engine.run(&stop).unwrap();

    assert!(!ran.exists());
}

// ------------------------------------------------- what cannot be read --

#[test]
fn an_unreadable_game_list_gives_an_unnamed_session_not_a_failure() {
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::Exited])
        .list_unreadable();
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    // Detected, unnamed, refined to nothing, ended: naming never gates a
    // session.
    assert_eq!(seen(&log), vec![Session::Playing(None), Session::Idle]);
}

#[test]
fn unreadable_counters_keep_the_first_name() {
    let stop = Arc::new(StopSignal::new().unwrap());
    // Two candidates, so the GPU is consulted -- and refuses.
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::Exited])
        .candidates(&[&[game(5, "first.exe"), game(10, "second.exe")]])
        .alive(&[5, 10])
        .counters_unreadable();
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(
        seen(&log),
        vec![Session::Playing(Some(game(5, "first.exe"))), Session::Idle]
    );
}

#[test]
fn the_writer_exit_is_recorded_whether_the_game_is_still_running_or_not() {
    // Both branches of the diagnostic line, through one script each. Nothing
    // observable beyond "it did not panic and the session closed", which is
    // what the line is for: a log, not a decision.
    for alive in [&[10][..], &[][..]] {
        let stop = Arc::new(StopSignal::new().unwrap());
        let sensor = Scripted::new(&stop)
            .writer(&[Some(7)])
            .waits(&[WaitOutcome::Exited])
            .candidates(&[&[game(10, "game.exe")]])
            .alive(alive);
        let (sink, log) = recorder();
        let mut engine = Engine::new(quick_config(), sensor).reporting_to(sink);
        engine.run(&stop).unwrap();
        assert_eq!(seen(&log).last(), Some(&Session::Idle));
    }
}
