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

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::{self, Config};
use crate::win::{SessionWindow, SingleInstance, StopSignal};
use crate::{engine, logging, tray, win};

/// Ceiling on how long `WM_ENDSESSION` holds the shutdown while the stop
/// actions run. `schtasks` returns in about 100 ms, so this is only here so a
/// wedged command cannot hold up the user's logoff indefinitely -- Windows has
/// its own, shorter patience anyway.
const SESSION_END_GRACE: Duration = Duration::from_secs(20);

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
    let _guards = logging::init(level, log_dir.as_deref(), console)?;
    // Installed as early as the log exists, so a panic anywhere after this
    // leaves a FATAL line behind rather than a process that simply vanished.
    logging::install_panic_hook();
    let _instance = SingleInstance::acquire("GameModeExecutor")?;

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
    // The marker lives next to the log: the one folder the watcher has already
    // proved it can write to, and where someone reading the log will find it.
    let marker_dir = log_dir.clone();
    let worker = std::thread::spawn(move || {
        let outcome = engine::Engine::new(config)
            .map(|engine| engine.reporting_to(sink))
            .map(|engine| match &marker_dir {
                Some(dir) => engine.remembering_in(dir),
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
