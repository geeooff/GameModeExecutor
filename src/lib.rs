//! GameModeExecutor: run configured executables when a game starts and stops.
//!
//! The pieces live in a library so the shipping binary and `presence-probe`
//! share one implementation — a measurement is only worth something if it
//! measures the code that will actually run.

#[cfg(not(windows))]
compile_error!("GameModeExecutor only targets Windows");

pub mod actions;
pub mod build_info;
pub mod cli;
pub mod config;
pub mod detect;
pub mod engine;
pub mod exit;
pub mod logging;
pub mod marker;
pub mod package;
pub mod purge;
pub mod registry;
pub mod sensor;
pub mod service;
pub mod shell;
pub mod task;
pub mod tray;
pub mod update;
pub mod win;
