//! Running the watcher, for both binaries.
//!
//! The engine runs on a worker thread and the main thread pumps messages for a
//! window that is never shown. The window is what the notification icon, the
//! theme broadcasts and the session-end handshake hang off; a Windows-subsystem
//! process without one hears nothing from the shell and is simply terminated.
//!
//! It was built to preserve what the console build was believed to do at
//! logoff -- `ctrlc`'s handler fires on every control event, so the stop
//! commands were *started* -- and a real logoff on 2026-09-16 showed that
//! starting them is not running them: a process created even one millisecond
//! after `WM_QUERYENDSESSION` dies with `STATUS_DLL_INIT_FAILED`. The handshake
//! still runs the stop commands, because it is right for a `Quit` and costs
//! nothing, but restoring the profile after a session end is Lot 9's marker
//! file, not this.
//!
//! `stop` is *Quit* from outside: `WM_CLOSE` on the session window, then a
//! wait on the single-instance mutex, which the watcher releases only after
//! its last log line. `purge` and the installer both use it, so the files
//! are never pulled from under a running watcher. With `StopReason::Handover`
//! the session window gets `WM_HANDOVER` instead, and a game session that
//! is open stays open for the watcher that follows.
//!
//! **The configuration is the supervisor's, not the engine's.** The worker
//! thread runs a supervisor: one engine per usable configuration, built on
//! the file as it is and stopped -- through the handover path, session kept
//! open -- when the file changes, so that the next engine reads the new one
//! and resumes what the last left. A file that cannot be used freezes the
//! program rather than stopping it: the icon shows the fault, nothing is
//! watched, and the next usable file starts an engine that settles the
//! open session the way a start does. Decided 2026-09-16, built
//! 2026-09-19; `docs/design/09-robustness.md` says why it is strict.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::config::{self, Config, FaultSink, LoadError, Report};
use crate::win::{SessionWindow, SingleInstance, StopReason, StopSignal};
use crate::{engine, logging, sensor, tray, update, win};

/// Ceiling on how long `WM_ENDSESSION` holds the shutdown while the stop
/// actions run. `schtasks` returns in about 100 ms, so this is only here so a
/// wedged command cannot hold up the user's logoff indefinitely -- Windows has
/// its own, shorter patience anyway.
const SESSION_END_GRACE: Duration = Duration::from_secs(20);

/// The single-instance mutex, session-local. The watcher holds it for its
/// whole life; `stop` and `purge` read it to know whether one is running and
/// when it has gone.
pub const INSTANCE: &str = "GameModeExecutor";

/// How long `stop` waits for the watcher to have gone. The stop commands run
/// on the way out, so this must outlast a slow one, and it only bounds a
/// watcher that is wedged.
const STOP_PATIENCE: Duration = Duration::from_secs(30);

/// Run the watcher until it is stopped.
///
/// The configuration is loaded here rather than by the caller, because a
/// file that cannot be used is not a reason to exit: the watcher starts,
/// shows the fault and waits for the file to change. `level` is the
/// command line's `--log-level`, which wins over the file's, now and at
/// every reload.
///
/// `console` says whether this process has a console: it gates both the log's
/// console layer and the Ctrl-C handler, neither of which means anything
/// without one.
pub fn serve(config_path: &Path, level: Option<&str>, console: bool) -> Result<()> {
    let (text, loaded) = load(config_path);
    // The watcher always keeps a log file. A windowless instance has nowhere
    // else to write, and a console one is usually left running unattended.
    // A file that cannot be read cannot say where, nor how long to keep it:
    // the defaults, then.
    let general = loaded
        .as_ref()
        .ok()
        .map(|config| config.general.clone())
        .unwrap_or_default();
    let log_dir = general.log_dir();
    let opened_at = level.map_or_else(|| general.log_level.clone(), str::to_owned);
    logging::init(&opened_at, log_dir.as_deref(), general.log_days, console)?;
    // Installed as early as the log exists, so a panic anywhere after this
    // leaves a FATAL line behind rather than a process that simply vanished.
    logging::install_panic_hook();
    let _instance = SingleInstance::acquire(INSTANCE)?;

    // Before any window exists, or the process stays DPI-unaware for its whole
    // life and the notification icon is built at the wrong size.
    win::declare_dpi_awareness();

    let stop = Arc::new(StopSignal::new()?);
    // Signalled by the worker once the engine has returned, which is after the
    // stop actions have run. WM_ENDSESSION waits on exactly this.
    let finished = Arc::new(StopSignal::new()?);

    let window =
        SessionWindow::create(Arc::clone(&stop), Arc::clone(&finished), SESSION_END_GRACE)?;
    let window_id = window.id();

    if console {
        let handler_stop = Arc::clone(&stop);
        ctrlc::set_handler(move || handler_stop.signal())
            .context("cannot install the Ctrl-C handler")?;
    }

    // The icon is a convenience, not the program. A shell that will not give us
    // one -- an unusual session, an Explorer that is not running -- is a reason
    // to say so and carry on watching, not to refuse to start.
    let targets = tray::Targets {
        config: config_path.to_path_buf(),
        log_dir: log_dir.clone().unwrap_or_default(),
    };
    if let Err(error) = tray::install(window_id, targets, Arc::clone(&stop)) {
        tracing::warn!(
            target: logging::target::WATCHER,
            error = %format!("{error:#}"),
            "No notification icon; the watcher runs without one"
        );
    }

    // The updater: reads what the last update left behind and gives the
    // menu its section. It never connects on its own.
    match update::Context::of_this_process(
        Some(Arc::clone(&stop)),
        Some(tray::update_sink(window_id)),
    ) {
        Ok(context) => update::start(context),
        Err(error) => tracing::warn!(
            target: logging::target::UPDATE,
            error = %format!("{error:#}"),
            "Updates are unavailable from the menu this session"
        ),
    }

    // The commit rides along as a field, so it is there at debug level when
    // someone is working out which build wrote a log they were sent, and out of
    // the way otherwise.
    tracing::info!(
        target: logging::target::WATCHER,
        commit = crate::build_info::COMMIT,
        "GameModeExecutor {} starting",
        crate::build_info::VERSION
    );

    // The engine reports session changes to the tray, which is how the icon,
    // the tooltip and the menu stay in step with each other and with reality;
    // the supervisor reports the configuration's faults the same way.
    let sink = tray::session_sink(window_id);
    let faults = tray::fault_sink(window_id);
    // State, so it lives with the local profile and not with the log, which
    // the user may have sent elsewhere and is entitled to empty.
    let marker = crate::marker::Marker::in_local_dir();
    let fault_marker = crate::marker::FaultMarker::in_local_dir();
    // One engine run stops on this; the process stops on `stop`, which it
    // answers to as well.
    let run_stop = Arc::new(StopSignal::child_of(&stop)?);
    // Watching the folder is a convenience over restarting; a folder that
    // cannot be watched says so and the file is read at the next start.
    let watcher = match config::watch(
        config_path.to_path_buf(),
        text,
        Arc::clone(&stop),
        Arc::clone(&run_stop),
    ) {
        Ok(watcher) => Some(watcher),
        Err(error) => {
            tracing::warn!(
                target: logging::target::WATCHER,
                error = %format!("{error:#}"),
                "The configuration's folder cannot be watched, so a change to the file \
                 takes effect at the next start"
            );
            None
        }
    };
    let mut supervised = Supervised {
        path: config_path.to_path_buf(),
        level: level.map(str::to_owned),
        applied_level: opened_at,
        log_dir: log_dir.clone(),
        log_days: general.log_days,
        sink,
        faults,
        marker,
        fault_marker,
    };
    let worker = std::thread::spawn(move || {
        // Once a start, off the window's thread: it reads a 2 MB file.
        crate::detect::hand_made::say_what_microsoft_now_covers();
        let outcome =
            sensor::Windows::new().and_then(|sensor| supervised.run(&sensor, loaded, &run_stop));
        // Order matters: release WM_ENDSESSION first, then wake the loop.
        finished.signal();
        win::wake_message_loop(window_id);
        outcome
    });

    win::run_message_loop();

    // Before anything else: the shell keeps a ghost icon until something hovers
    // over it otherwise, which looks like the program is still there.
    tray::uninstall();

    // The loop returns once the worker posted its message, or once the window
    // was destroyed. Signalling again is harmless and covers the second case.
    stop.signal();
    let outcome = worker
        .join()
        .map_err(|_| anyhow::anyhow!("the watcher thread panicked"))?;
    if let Some(watcher) = watcher {
        // Parked on the stop event too, so this is immediate.
        let _ = watcher.join();
    }
    outcome?;

    tracing::info!(target: logging::target::WATCHER, "Stopped");
    Ok(())
}

/// The file's text, when it can be read, and what it parses to.
///
/// The text goes to the folder watch, so that the first change it reports
/// is a change to what is running and not to what it read itself a moment
/// later.
fn load(path: &Path) -> (Option<String>, Result<Config, LoadError>) {
    match Config::read(path) {
        Ok(text) => {
            let parsed = Config::parse(&text, path);
            (Some(text), parsed)
        }
        Err(error) => (None, Err(error)),
    }
}

/// What the supervisor keeps across engines.
struct Supervised {
    path: std::path::PathBuf,
    /// The command line's level, which wins over the file's.
    level: Option<String>,
    /// The level the log is at, so a reload that keeps it is silent.
    applied_level: String,
    /// Where the log was opened; a file that moves it is told to wait.
    log_dir: Option<std::path::PathBuf>,
    /// How many days the log keeps; a change waits too.
    log_days: u32,
    sink: engine::SessionSink,
    faults: FaultSink,
    marker: Option<crate::marker::Marker>,
    /// Remembers a fault across processes, so a start on a file fixed while
    /// the watcher was stopped still says the fault is over.
    fault_marker: Option<crate::marker::FaultMarker>,
}

impl Supervised {
    /// One engine per usable configuration, until the process stops.
    ///
    /// `stop` is the run's signal: set by the folder watch for a reload, by
    /// the process-wide stop for anything else. An engine that returns on
    /// a reload has left an open session in the marker, the way a handover
    /// does, and the next engine's recovery settles it: resumed when the
    /// game is still on, its stop commands run when it is not. A file that
    /// cannot be used is shown and waited on; nothing runs meanwhile, not
    /// even the stop commands of a session that ends, which is what
    /// "disabled outright" means and why the marker is the right memory.
    ///
    /// The tray is told which transition each read is -- a fault, the end
    /// of one, or a usable file that was usable before -- because it says
    /// the first two with a notification and not the third. "Before"
    /// includes the last process: the fault marker carries it across.
    fn run(
        &mut self,
        sensor: &sensor::Windows,
        mut loaded: Result<Config, LoadError>,
        stop: &StopSignal,
    ) -> Result<()> {
        let mut first = true;
        let mut faulty = false;
        loop {
            match loaded {
                Ok(config) => {
                    let remembered = match &self.fault_marker {
                        Some(marker) => marker.clear().unwrap_or_else(|error| {
                            tracing::warn!(
                                target: logging::target::WATCHER,
                                path = %marker.path().display(),
                                error = %error,
                                "Cannot remove the configuration-fault marker"
                            );
                            false
                        }),
                        None => false,
                    };
                    (self.faults)(if faulty || remembered {
                        Report::Restored
                    } else {
                        Report::Usable
                    });
                    faulty = false;
                    // The log first, so the line below is written the way
                    // the new file asks.
                    self.follow(&config);
                    if !first {
                        tracing::info!(
                            target: logging::target::WATCHER,
                            path = %self.path.display(),
                            "Configuration reloaded"
                        );
                    }
                    let mut engine =
                        engine::Engine::new(config, sensor).reporting_to(Arc::clone(&self.sink));
                    if let Some(marker) = &self.marker {
                        engine = engine.remembering(marker.clone());
                    }
                    engine.run(stop)?;
                }
                Err(fault) => {
                    (self.faults)(Report::Faulty(&fault));
                    faulty = true;
                    if let Some(marker) = &self.fault_marker
                        && let Err(error) = marker.note(&fault.summary(), &logging::local_now())
                    {
                        tracing::warn!(
                            target: logging::target::WATCHER,
                            path = %marker.path().display(),
                            error = %error,
                            "Cannot write the configuration-fault marker, so a start after the \
                             fix will not say the fault is over"
                        );
                    }
                    tracing::error!(
                        target: logging::target::WATCHER,
                        path = %self.path.display(),
                        "The configuration cannot be used, so nothing is watched until it is \
                         fixed: {}",
                        fault.summary()
                    );
                    // Frozen: the folder watch or the process-wide stop
                    // ends this, nothing else.
                    stop.wait();
                }
            }
            if !stop.take_reload() {
                return Ok(());
            }
            first = false;
            loaded = load(&self.path).1;
        }
    }

    /// Apply what a configuration says about the log itself: the level
    /// follows live unless the command line fixed it; the folder cannot
    /// move under an open file, and the days kept are the appender's, built
    /// once: both wait for the next start.
    fn follow(&mut self, config: &Config) {
        if self.level.is_none() && config.general.log_level != self.applied_level {
            logging::set_level(&config.general.log_level);
            self.applied_level = config.general.log_level.clone();
        }
        let wanted = config.general.log_dir();
        if wanted != self.log_dir {
            tracing::warn!(
                target: logging::target::WATCHER,
                wanted = wanted.as_deref().map(|dir| dir.display().to_string()),
                "log_dir changed; the log moves there at the next start"
            );
        }
        if config.general.log_days != self.log_days {
            tracing::warn!(
                target: logging::target::WATCHER,
                wanted = config.general.log_days,
                "log_days changed; the log keeps that many days from the next start"
            );
        }
    }
}

/// What `stop` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// A watcher was running; it has gone.
    Stopped,
    /// None was running.
    NotRunning,
}

/// Ask the running watcher to stop and wait for it to have gone. With
/// `Restore` that is *Quit*: mid-game the stop commands run. With
/// `Handover` an open session is left in the marker for the watcher that
/// follows. No watcher is not an error: the caller wanted none running, and
/// none is.
///
/// A watcher that is still starting holds the mutex before it has a window,
/// so the close is retried until the mutex is free. Logged at `info` under
/// `setup`, whether a person or the installer asked.
pub fn stop(reason: StopReason) -> Result<Stopped> {
    if !SingleInstance::is_held(INSTANCE) {
        tracing::info!(target: logging::target::SETUP, "No watcher was running");
        return Ok(Stopped::NotRunning);
    }
    let asked = Instant::now();
    loop {
        // May find no window yet, or none any more: the mutex is the verdict.
        let _ = win::close_session_window(reason);
        if !SingleInstance::is_held(INSTANCE) {
            match reason {
                StopReason::Restore => tracing::info!(
                    target: logging::target::SETUP,
                    waited = ?asked.elapsed(),
                    "Watcher stopped, as asked"
                ),
                StopReason::Handover => tracing::info!(
                    target: logging::target::SETUP,
                    waited = ?asked.elapsed(),
                    "Watcher stopped, as asked; a game session that was open waits for the next one"
                ),
                // Refused by `close_session_window` above, before any wait.
                StopReason::Reload => unreachable!("a reload is never asked of another process"),
            }
            return Ok(Stopped::Stopped);
        }
        anyhow::ensure!(
            asked.elapsed() < STOP_PATIENCE,
            "the watcher did not stop within {STOP_PATIENCE:?}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}
