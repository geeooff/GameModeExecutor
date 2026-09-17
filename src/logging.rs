//! The logging chain: categories, filtering, and how one line is rendered.
//!
//! The log has two readers and must serve both from a single set of events.
//!
//! Someone who just wants to know what happened reads at `info`, where the
//! only lines are the ones this program exists for: a game was detected, it was
//! named, it is gone, the watcher started or stopped. A technician reads the
//! same log at `debug`, and gets those lines *annotated* — process ids, paths,
//! error codes — plus the reasoning behind them.
//!
//! That is why an event carries a plain sentence as its message and everything
//! technical as structured fields: the fields are printed only when the level
//! asks for them. One event, two readings, nothing written twice.

use std::fmt::Write as _;
use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Context, Result};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};
use windows::Win32::System::SystemInformation::GetLocalTime;

/// The categories a line can belong to.
///
/// These replace Rust module paths, which said `game_mode_executor::engine` to
/// a reader who has never heard of a module. Every logging call must name one:
/// the level filter is built from this list, so an event with any other target
/// matches no directive and is silently dropped. `every_log_site_declares_a_category`
/// guards that.
///
/// Deliberately three words. More will be added when `config` or the tray start
/// logging, and not before — inventing vocabulary for code that does not log
/// yet is how a glossary stops being read.
pub mod target {
    /// The program's own life: starting, stopping, what it is watching.
    pub const WATCHER: &str = "watcher";
    /// Detecting a game, naming it, losing it. The reason this program exists.
    pub const GAME: &str = "game";
    /// Running the executables from the configuration.
    pub const COMMANDS: &str = "commands";
    /// Setting the program up and taking it down: the starter configuration,
    /// the logon task. Written by the commands and by the installer alike,
    /// so the log says who did what to this machine and when.
    pub const SETUP: &str = "setup";

    /// Every category, in the order they appear in a session.
    pub const ALL: &[&str] = &[WATCHER, GAME, COMMANDS, SETUP];

    /// Width of the category column, so messages line up whatever the category.
    pub(super) const WIDTH: usize = 8;
}

/// Timestamps in the reader's own time zone.
///
/// The default is UTC, which files an event that happened at 00:46 under the
/// previous day at 22:46. For a log whose only purpose is to be read by the
/// person who just played a game, that is a defect. `GetLocalTime` avoids both
/// the `time` crate's local-offset caveats and an extra feature flag, and
/// matches the format `presence-probe` already writes.
struct LocalTimestamp;

impl FormatTime for LocalTimestamp {
    fn format_time(&self, writer: &mut Writer<'_>) -> std::fmt::Result {
        // SAFETY: `GetLocalTime` takes no input and only returns a struct.
        let now = unsafe { GetLocalTime() };
        write!(
            writer,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond, now.wMilliseconds
        )
    }
}

/// The local time as the log writes it, to the second. For anything that
/// wants to be read next to the log and agree with it.
pub fn local_now() -> String {
    // SAFETY: `GetLocalTime` takes no input and only returns a struct.
    let now = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond
    )
}

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn level_colour(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "\x1b[31m",
        Level::WARN => "\x1b[33m",
        Level::INFO => "\x1b[32m",
        Level::DEBUG => "\x1b[34m",
        Level::TRACE => DIM,
    }
}

/// One rendered line: `timestamp LEVEL category message [fields]`.
struct Line {
    /// Print the structured fields. They are the technical annex to a line, so
    /// they appear only when the reader asked for that level of detail.
    verbose: bool,
    ansi: bool,
}

impl<S, N> FormatEvent<S, N> for Line
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();

        if self.ansi {
            write!(writer, "{DIM}")?;
        }
        LocalTimestamp.format_time(&mut writer)?;
        if self.ansi {
            write!(writer, "{RESET}")?;
        }

        if self.ansi {
            write!(
                writer,
                " {}{:>5}{RESET}",
                level_colour(meta.level()),
                meta.level()
            )?;
        } else {
            write!(writer, " {:>5}", meta.level())?;
        }

        let category = format!("{:<width$}", meta.target(), width = target::WIDTH);
        if self.ansi {
            write!(writer, "  {DIM}{category}{RESET}")?;
        } else {
            write!(writer, "  {category}")?;
        }

        let mut collected = Collected::default();
        event.record(&mut collected);
        write!(writer, "  {}", collected.message)?;
        if self.verbose && !collected.fields.is_empty() {
            if self.ansi {
                write!(writer, "  {DIM}{}{RESET}", collected.fields)?;
            } else {
                write!(writer, "  {}", collected.fields)?;
            }
        }
        writeln!(writer)
    }
}

/// Splits an event into the sentence a person reads and the fields a
/// technician needs.
#[derive(Default)]
struct Collected {
    message: String,
    fields: String,
}

impl Collected {
    fn push_field(&mut self, name: &str, value: std::fmt::Arguments<'_>) {
        if !self.fields.is_empty() {
            self.fields.push(' ');
        }
        let _ = write!(self.fields, "{name}={value}");
    }
}

impl Visit for Collected {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            self.push_field(field.name(), format_args!("{value:?}"));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            self.push_field(field.name(), format_args!("{value:?}"));
        }
    }
}

/// The one file the log is written to. Named here rather than inline because
/// the tray's "Open log" entry has to point at the same one.
pub const LOG_FILE_NAME: &str = "gamemode-executor.log";

/// Record a panic in the log before the process dies.
///
/// `tracing` has five levels and `FATAL` is not one of them. Rather than build
/// a sixth, the word goes in the message: the level stays `error`, which is
/// what every filter and every reader already understands, and the line still
/// says plainly that this was the end rather than a command that failed.
///
/// This works under `panic = "abort"` -- which the release profile uses --
/// because the hook runs before the process is killed. It relies on the log
/// being written synchronously; see [`init`] for why it is.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map_or_else(|| "an unknown location".to_owned(), ToString::to_string);
        tracing::error!(
            target: target::WATCHER,
            "FATAL: GameModeExecutor {} panicked at {location}: {}",
            crate::build_info::VERSION,
            payload(info)
        );
        // Still print it: a console build has someone watching, and the default
        // hook says more than this line does.
        previous(info);
    }));
}

/// The panic message, which arrives as one of two types and nothing else.
fn payload(info: &std::panic::PanicHookInfo<'_>) -> String {
    if let Some(text) = info.payload().downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = info.payload().downcast_ref::<String>() {
        text.clone()
    } else {
        "no message".to_owned()
    }
}

/// One directive per category, so the filter can never fall out of step with
/// the vocabulary. Listing them by hand is how a renamed category becomes a
/// silently empty log.
fn directives(level: &str) -> String {
    target::ALL
        .iter()
        .map(|category| format!("{category}={level}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Whether to print the fields, which is a question about what the reader
/// asked for rather than about any one event. `RUST_LOG` wins when set,
/// because it also wins over `level` for the filter itself.
fn verbose_for(level: &str) -> bool {
    let asked = std::env::var("RUST_LOG").unwrap_or_else(|_| level.to_owned());
    let asked = asked.to_ascii_lowercase();
    asked.contains("debug") || asked.contains("trace")
}

/// Initialise logging. `RUST_LOG` overrides `level` when set.
///
/// `console` says whether this process has a console at all, which is a
/// property of the binary rather than a preference: `gamemode-executor` is a
/// console program and always passes `true`, `gamemode-executorw` has no
/// console and passes `false`.
///
/// It must never become a user-facing switch again. It was one once -- tied to
/// `--hidden` -- and that conflated two unrelated things: whether a window is
/// visible, and whether anything is written to it. The result was a visible
/// window that stayed blank forever.
pub fn init(level: &str, log_dir: Option<&Path>, console: bool) -> Result<()> {
    let verbose = verbose_for(level);
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(directives(level)));

    let file_layer = match log_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create log directory `{}`", dir.display()))?;
            // One file, not a daily rotation. This log gains a handful of lines
            // per game session, and rotation only bought filenames dated in UTC
            // -- the very confusion the local timestamps above remove.
            //
            // Written synchronously, on purpose. Buffering it on a background
            // thread would save nothing at this volume and costs the only lines
            // that really matter: the release profile aborts on panic, so
            // nothing is dropped and a buffered crash report is never flushed.
            let path = dir.join(LOG_FILE_NAME);
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("cannot open the log file `{}`", path.display()))?;
            Some(
                fmt::layer()
                    .event_format(Line {
                        verbose,
                        ansi: false,
                    })
                    .with_writer(std::sync::Mutex::new(file)),
            )
        }
        None => None,
    };

    // Colours only when a person is really at a terminal. Unconditional ANSI
    // writes escape codes into whatever captures the output -- `run > log.txt`,
    // or a pipe -- where they are noise rather than colour.
    let console_layer = console.then(|| {
        fmt::layer().event_format(Line {
            verbose,
            ansi: std::io::stdout().is_terminal(),
        })
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Buffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buffer {
        type Writer = Buffer;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn render(verbose: bool, emit: impl FnOnce()) -> String {
        let buffer = Buffer::default();
        let layer = fmt::layer()
            .event_format(Line {
                verbose,
                ansi: false,
            })
            .with_writer(buffer.clone());
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, emit);
        let bytes = buffer.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn a_plain_reader_sees_the_sentence_and_not_the_fields() {
        let line = render(false, || {
            tracing::info!(target: target::GAME, pid = 14552, matched_by = "exe path", "Game detected: bf6.exe");
        });
        assert!(line.contains("Game detected: bf6.exe"), "{line}");
        assert!(!line.contains("14552"), "fields must stay hidden: {line}");
        assert!(
            !line.contains("matched_by"),
            "fields must stay hidden: {line}"
        );
    }

    #[test]
    fn a_technician_sees_the_same_line_annotated() {
        let line = render(true, || {
            tracing::info!(target: target::GAME, pid = 14552, matched_by = "exe path", "Game detected: bf6.exe");
        });
        assert!(line.contains("Game detected: bf6.exe"), "{line}");
        assert!(line.contains("pid=14552"), "{line}");
        assert!(line.contains(r#"matched_by="exe path""#), "{line}");
    }

    /// Optional data is the common case here -- a game Windows tracks but does
    /// not describe has no pid and no path -- so pin down how it renders
    /// rather than discovering `pid=Some(14552)` in a log someone is reading.
    #[test]
    fn an_optional_field_renders_as_its_value_not_as_an_option() {
        let present: Option<u32> = Some(14552);
        let line = render(true, || {
            tracing::info!(target: target::GAME, pid = present, "Game detected: bf6.exe");
        });
        assert!(line.contains("pid=14552"), "{line}");
        assert!(!line.contains("Some("), "{line}");
    }

    #[test]
    fn the_category_is_a_word_not_a_module_path() {
        let line = render(false, || {
            tracing::info!(target: target::WATCHER, "GameModeExecutor 0.1.0 starting");
        });
        assert!(line.contains("watcher"), "{line}");
        assert!(
            !line.contains("::"),
            "no module paths in a log line: {line}"
        );
    }

    #[test]
    fn messages_line_up_whatever_the_category() {
        let short = render(false, || tracing::info!(target: target::GAME, "x"));
        let long = render(false, || tracing::info!(target: target::COMMANDS, "x"));
        let column = |line: &str| line.rfind('x').unwrap();
        assert_eq!(column(&short), column(&long), "{short}{long}");
    }

    /// The line a crash leaves behind is the only thing anyone will have, so
    /// it is worth knowing it is written and what it says.
    ///
    /// This runs under unwinding, which the test profile uses. That the hook
    /// also runs under `panic = "abort"` -- the release profile, where nothing
    /// is dropped and the log's background writer never flushes -- was checked
    /// separately with a standalone binary built `-C panic=abort`, and is why
    /// the hook writes to the file directly instead of going through `tracing`.
    /// The line a crash leaves behind is the only thing anyone will have, so
    /// it is worth knowing it is written and what it says.
    ///
    /// This runs under unwinding, which the test profile uses. That the hook
    /// also runs under `panic = "abort"` -- the release profile -- was checked
    /// separately with a standalone binary built `-C panic=abort`, and is why
    /// the log is written synchronously rather than buffered on a thread that
    /// never gets to flush.
    #[test]
    fn a_panic_is_recorded_before_the_process_dies() {
        let written = render(false, || {
            install_panic_hook();
            let _ = std::panic::catch_unwind(|| panic!("a deliberate test panic"));
            let _ = std::panic::take_hook();
        });

        assert!(written.contains("FATAL"), "{written}");
        assert!(written.contains("a deliberate test panic"), "{written}");
        // Which build died matters as much as that it died.
        assert!(written.contains(crate::build_info::VERSION), "{written}");
        // And it belongs in the same category as the rest of the watcher's life.
        assert!(written.contains(target::WATCHER), "{written}");
    }

    #[test]
    fn the_filter_covers_every_category() {
        let directives = directives("debug");
        for category in target::ALL {
            assert!(
                directives.contains(&format!("{category}=debug")),
                "`{category}` is missing from `{directives}`, so its lines would vanish"
            );
        }
    }

    /// An event whose target is not in `target::ALL` matches no directive and
    /// is dropped without a word. That failure is invisible at runtime -- the
    /// log simply lacks a line nobody knows to look for -- so it is caught
    /// here instead.
    #[test]
    fn every_log_site_declares_a_category() {
        fn visit(dir: &Path, offenders: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).expect("cannot read src") {
                let path = entry.expect("cannot read entry").path();
                if path.is_dir() {
                    visit(&path, offenders);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "rs") {
                    continue;
                }
                let source = std::fs::read_to_string(&path).expect("cannot read source");
                inspect(&source, &path, offenders);
            }
        }

        /// Scans the whole text rather than line by line: a macro call is
        /// routinely spread over several lines, with `target:` on the second.
        fn inspect(source: &str, path: &Path, offenders: &mut Vec<String>) {
            for level in ["info", "warn", "error", "debug", "trace"] {
                let opening = format!("tracing::{level}!(");
                let mut searched = 0;
                while let Some(found) = source[searched..].find(&opening) {
                    let after = searched + found + opening.len();
                    if !source[after..].trim_start().starts_with("target:") {
                        let line = source[..after].lines().count();
                        offenders.push(format!("{}:{line}", path.display()));
                    }
                    searched = after;
                }
            }
        }

        let mut offenders = Vec::new();
        visit(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut offenders,
        );
        assert!(
            offenders.is_empty(),
            "these log lines would be filtered out for having no category:\n{}",
            offenders.join("\n")
        );
    }
}
