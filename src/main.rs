//! The console executable: what people type.
//!
//! Every command lives in the library's `cli` module, shared with the
//! windowless twin; this file only adds what a console is for -- an error
//! printed where the person can read it, and an exit code a shell waits for.

use clap::Parser;

use game_mode_executor::{cli, exit};

fn main() -> std::process::ExitCode {
    match cli::run(cli::Cli::parse(), true) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            std::process::ExitCode::from(exit::code_for(&error))
        }
    }
}
