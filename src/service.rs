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

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::config::{self, Config};
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
/// `console` says whether this process has a console: it gates both the log's
/// console layer and the Ctrl-C handler, neither of which means anything
/// without one.
pub fn serve(
    config: Config,
    config_path: &std::path::Path,
    level: &str,
    console: bool,
) -> Result<()> {
    // The watcher always keeps a log file. A windowless instance has nowhere
    // else to write, and a console one is usually left running unattended.
    let log_dir = config
        .general
        .log_dir
        .clone()
        .or_else(|| config::local_dir().map(|dir| dir.join("logs")));
    logging::init(level, log_dir.as_deref(), console)?;
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
        log: log_dir
            .clone()
            .unwrap_or_default()
            .join(logging::LOG_FILE_NAME),
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
    // the tooltip and the menu stay in step with each other and with reality.
    let sink = tray::session_sink(window_id);
    let worker_stop = Arc::clone(&stop);
    // State, so it lives with the local profile and not with the log, which
    // the user may have sent elsewhere and is entitled to empty.
    let marker = crate::marker::Marker::in_local_dir();
    let worker = std::thread::spawn(move || {
        let outcome = sensor::Windows::new()
            .map(|sensor| engine::Engine::new(config, sensor).reporting_to(sink))
            .map(|engine| match marker {
                Some(marker) => engine.remembering(marker),
                None => engine,
            })
            .and_then(|mut engine| engine.run(&worker_stop));
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
    outcome?;

    tracing::info!(target: logging::target::WATCHER, "Stopped");
    Ok(())
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
