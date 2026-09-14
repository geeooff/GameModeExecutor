//! Running the watcher, for both binaries.
//!
//! The engine runs on a worker thread and the main thread pumps messages for a
//! window that is never shown. That inversion is not for the sake of a future
//! tray icon: it is what keeps the program behaving as it did as a console
//! program. `ctrlc`'s Windows handler fires on every control event, logoff and
//! shutdown included, so the console build restored the fan profile when the
//! user logged out mid-game. A Windows-subsystem process with no window gets
//! none of that and is simply terminated. The window earns its place by
//! answering `WM_QUERYENDSESSION`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::{self, Config};
use crate::win::{SessionWindow, SingleInstance, StopSignal};
use crate::{engine, logging, win};

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
pub fn serve(config: Config, level: &str, console: bool) -> Result<()> {
    // The watcher always keeps a log file. A windowless instance has nowhere
    // else to write, and a console one is usually left running unattended.
    let log_dir = config
        .general
        .log_dir
        .clone()
        .or_else(|| config::local_dir().map(|dir| dir.join("logs")));
    let _guards = logging::init(level, log_dir.as_deref(), console)?;
    let _instance = SingleInstance::acquire("GameModeExecutor")?;

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

    tracing::info!(
        target: logging::target::WATCHER,
        "GameModeExecutor {} starting",
        env!("CARGO_PKG_VERSION")
    );

    let worker_stop = Arc::clone(&stop);
    let worker = std::thread::spawn(move || {
        let outcome = engine::Engine::new(config).and_then(|mut engine| engine.run(&worker_stop));
        // Order matters: release WM_ENDSESSION first, then wake the loop.
        finished.signal();
        win::wake_message_loop(window_id);
        outcome
    });

    win::run_message_loop();

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
