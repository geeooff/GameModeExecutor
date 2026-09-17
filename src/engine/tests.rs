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
    /// Answers to `writer_pid`, one per call. When they run out the stop is
    /// signalled, which is how `run` is made to return.
    writer: RefCell<VecDeque<Option<u32>>>,
    /// Answers to `wait_for_writer_exit`, one per call.
    waits: RefCell<VecDeque<WaitOutcome>>,
    /// Answers to `candidates`, one per call; the last one repeats.
    candidates: RefCell<VecDeque<Vec<GameSignal>>>,
    alive: HashSet<u32>,
    load: HashMap<u32, f64>,
    /// `candidates` fails, as it does when the list cannot be read.
    list_unreadable: bool,
    /// `rendering_load` fails, as it does on an account that may not read
    /// the counters.
    counters_unreadable: bool,
    stop: Arc<StopSignal>,
}

impl Scripted {
    fn new(stop: &Arc<StopSignal>) -> Self {
        Self {
            writer: RefCell::new(VecDeque::new()),
            waits: RefCell::new(VecDeque::new()),
            candidates: RefCell::new(VecDeque::new()),
            alive: HashSet::new(),
            load: HashMap::new(),
            list_unreadable: false,
            counters_unreadable: false,
            stop: Arc::clone(stop),
        }
    }

    fn list_unreadable(mut self) -> Self {
        self.list_unreadable = true;
        self
    }

    fn counters_unreadable(mut self) -> Self {
        self.counters_unreadable = true;
        self
    }

    fn writer(self, answers: &[Option<u32>]) -> Self {
        self.writer.borrow_mut().extend(answers.iter().copied());
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

    fn rendering(mut self, load: &[(u32, f64)]) -> Self {
        self.load.extend(load.iter().copied());
        self
    }
}

impl Sensor for Scripted {
    fn writer_pid(&self) -> Option<u32> {
        match self.writer.borrow_mut().pop_front() {
            Some(answer) => answer,
            None => {
                self.stop.signal();
                None
            }
        }
    }

    fn wait_for_writer_exit(
        &self,
        _pid: u32,
        _stop: &StopSignal,
        _timeout: Option<Duration>,
    ) -> Result<WaitOutcome> {
        Ok(self
            .waits
            .borrow_mut()
            .pop_front()
            .expect("the script ran out of waits"))
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
        Ok(self.load.clone())
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
        .waits(&[WaitOutcome::WriterExited])
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
        .waits(&[WaitOutcome::WriterExited])
        .candidates(&[&[]]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(seen(&log), vec![Session::Playing(None), Session::Idle]);
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
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::WriterExited])
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
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::WriterExited])
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
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::WriterExited])
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

#[test]
fn nothing_rendering_keeps_the_first_name() {
    // A game still on its loading screen: the one attempt is spent and the
    // name stays. Recorded as a known margin in the design record.
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::WriterExited])
        .candidates(&[&[game(5, "anticheat.exe"), game(10, "bf6.exe")]])
        .alive(&[5, 10])
        .rendering(&[(5, 0.0), (10, 0.0)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(
        seen(&log),
        vec![
            Session::Playing(Some(game(5, "anticheat.exe"))),
            Session::Idle
        ]
    );
}

#[test]
fn a_confirmed_name_is_not_reported_again() {
    let stop = Arc::new(StopSignal::new().unwrap());
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::WriterExited])
        .candidates(&[&[game(10, "bf6.exe"), game(5, "anticheat.exe")]])
        .alive(&[5, 10])
        .rendering(&[(10, 90.0)]);
    let (sink, log) = recorder();

    let mut engine = Engine::new(quick_config(), sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    // Two changes only: the start and the stop. The confirmation is a log
    // line, not a report, or the tray would redraw the same icon.
    assert_eq!(seen(&log).len(), 2);
}

#[test]
fn refinement_is_skipped_when_identify_after_is_zero() {
    let stop = Arc::new(StopSignal::new().unwrap());
    // No TimedOut in the script: with identify_after = 0 the engine never
    // asks for one, so a script that offered it would go unconsumed.
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7)])
        .waits(&[WaitOutcome::WriterExited])
        .candidates(&[&[game(5, "first.exe")]]);
    let (sink, log) = recorder();
    let mut config = quick_config();
    config.detection.identify_after = Duration::ZERO;

    let mut engine = Engine::new(config, sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    assert_eq!(seen(&log).len(), 2);
}

// ------------------------------------------------- the writer blinking --

#[test]
fn a_writer_that_comes_back_within_the_grace_keeps_the_session_open() {
    let stop = Arc::new(StopSignal::new().unwrap());
    // Writer 7 exits, 8 appears while the grace runs, then 8 exits for good.
    let sensor = Scripted::new(&stop)
        .writer(&[Some(7), Some(8)])
        .waits(&[WaitOutcome::WriterExited, WaitOutcome::WriterExited])
        .candidates(&[&[game(10, "game.exe")]]);
    let (sink, log) = recorder();
    let mut config = quick_config();
    config.detection.stop_delay = Duration::from_millis(200);

    let mut engine = Engine::new(config, sensor).reporting_to(sink);
    engine.run(&stop).unwrap();

    // One session, not two: the blink was absorbed.
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
    assert!(seen(&log).is_empty(), "recovery is not a session");
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
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::WriterExited])
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
        .waits(&[WaitOutcome::TimedOut, WaitOutcome::WriterExited])
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
            .waits(&[WaitOutcome::WriterExited])
            .candidates(&[&[game(10, "game.exe")]])
            .alive(alive);
        let (sink, log) = recorder();
        let mut engine = Engine::new(quick_config(), sensor).reporting_to(sink);
        engine.run(&stop).unwrap();
        assert_eq!(seen(&log).last(), Some(&Session::Idle));
    }
}
