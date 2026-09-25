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
//!
//! The file turns over each day, through `tracing-appender`, decided
//! 2026-09-24 in `docs/design/17-log-rotation.md` over code of our own: one
//! file a day, `gamemode-executor.YYYY-MM-DD.log`, the day being UTC's --
//! the library's, with no way to ask for local time -- and the last
//! `log_days` of them kept. The appender turns over at the first line after
//! midnight UTC, in the running process: no restart, measured in the spike
//! the same page records.

use std::fmt::Write as _;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result};
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::{FormatTime, LocalTime};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::{LookupSpan, Registry};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt, reload};

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
    /// Looking for, fetching and installing a newer release: the one thing
    /// in the program that touches a network, so every step of it is
    /// written down.
    pub const UPDATE: &str = "update";

    /// Every category, in the order they appear in a session.
    pub const ALL: &[&str] = &[WATCHER, GAME, COMMANDS, SETUP, UPDATE];

    /// Width of the category column, so messages line up whatever the category.
    pub(super) const WIDTH: usize = 8;
}

/// A line's timestamp, in the reader's own time zone, to the millisecond.
///
/// The default is UTC, which files an event that happened at 00:46 under the
/// previous day at 22:46. For a log whose only purpose is to be read by the
/// person who just played a game, that is a defect. `tracing-subscriber`'s
/// `LocalTime` writes it, through the `time` crate: its local-offset
/// caveat is Unix's, and on Windows it asks
/// `SystemTimeToTzSpecificLocalTime`, which is thread-safe -- checked
/// 2026-09-25, `docs/design/17-log-rotation.md`. It matches the format
/// `presence-probe` writes.
const LINE_TIME: &[BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]");

/// The local time as the log writes it, to the second. For anything that
/// wants to be read next to the log and agree with it.
pub fn local_now() -> String {
    // Only Unix can fail to tell the offset; UTC would at least be a time.
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    now.format(format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second]"
    ))
    .unwrap_or_default()
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
    /// Shared with [`set_level`], which moves it with the level.
    verbose: Arc<AtomicBool>,
    ansi: bool,
}

impl Line {
    fn verbose(&self) -> bool {
        self.verbose.load(Ordering::Relaxed)
    }
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
        LocalTime::new(LINE_TIME).format_time(&mut writer)?;
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
        if self.verbose() && !collected.fields.is_empty() {
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

/// The start and the end of every log file's name; the appender puts the
/// date between them. `log` last, so a double-click opens it as text.
const LOG_PREFIX: &str = "gamemode-executor";
const LOG_SUFFIX: &str = "log";

/// Whether `name` is one of the log's files, by the rule `tracing-appender`
/// prunes by: this prefix and this suffix. The single file of earlier
/// versions, `gamemode-executor.log`, is one too: the appender counts it
/// among the files it keeps and deletes it when it is the oldest.
pub fn is_log_file(name: &str) -> bool {
    name.starts_with(LOG_PREFIX) && name.ends_with(LOG_SUFFIX)
}

/// Every log file in `dir`, in no particular order. For `purge`, which
/// removes them all.
pub fn log_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| entry.file_name().to_str().is_some_and(is_log_file))
        .map(|entry| entry.path())
        .collect()
}

/// The file being written: the log file modified last.
///
/// `RollingFileAppender` does not say which file it writes, and working
/// its name out here would copy its date format and its clock. It opens a
/// file only to write a line in it, though, so the one written last is the
/// one in use -- by whichever process wrote last, which is the same file.
pub fn current_log(dir: &Path) -> Option<PathBuf> {
    log_files(dir)
        .into_iter()
        .filter_map(|path| {
            let modified = path.metadata().and_then(|meta| meta.modified()).ok()?;
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

/// What *Open log* opens: the file being written, or the folder when it
/// holds no log file at all.
pub fn to_open(dir: &Path) -> PathBuf {
    current_log(dir).unwrap_or_else(|| dir.to_path_buf())
}

/// The appender the file layer writes through.
///
/// `rotation` is [`Rotation::DAILY`] everywhere but in `presence-probe
/// log-turnover`, which runs the same appender turning over each minute:
/// the library's rotations differ in their period and nothing else.
///
/// It keeps one file more than `log_days`. An appender built on a day that
/// has its file already -- any command, any restart -- prunes to one below
/// its maximum, to make room for a file it then does not create; so with
/// `log_days + 1` the folder holds `log_days` or `log_days + 1` files, never
/// fewer. Measured in the spike, `docs/design/17-log-rotation.md`.
pub fn appender(dir: &Path, log_days: u32, rotation: Rotation) -> Result<RollingFileAppender> {
    RollingFileAppender::builder()
        .rotation(rotation)
        .filename_prefix(LOG_PREFIX)
        .filename_suffix(LOG_SUFFIX)
        .max_log_files(log_days as usize + 1)
        .build(dir)
        .with_context(|| format!("cannot open the log in `{}`", dir.display()))
}

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

/// The two things a change of `log_level` moves after `init`: the filter,
/// through its reload handle, and whether the fields are printed.
struct Live {
    filter: reload::Handle<EnvFilter, Registry>,
    verbose: Arc<AtomicBool>,
    /// `RUST_LOG` was set at start, and keeps winning.
    from_env: bool,
}

static LIVE: OnceLock<Live> = OnceLock::new();

/// Change the level after `init`, when the configuration's `log_level`
/// changed under a running watcher. `RUST_LOG`, when set, still wins, as it
/// did at start.
pub fn set_level(level: &str) {
    let Some(live) = LIVE.get() else {
        return;
    };
    if live.from_env {
        tracing::debug!(
            target: target::WATCHER,
            level,
            "log_level changed, but RUST_LOG is set and keeps deciding"
        );
        return;
    }
    match live.filter.reload(EnvFilter::new(directives(level))) {
        Ok(()) => {
            live.verbose.store(verbose_for(level), Ordering::Relaxed);
            tracing::debug!(target: target::WATCHER, level, "Log level changed");
        }
        Err(error) => tracing::warn!(
            target: target::WATCHER,
            level,
            error = %error,
            "The log level could not be changed; the previous one stays"
        ),
    }
}

/// Initialise logging. `RUST_LOG` overrides `level` when set; `log_days`
/// is how many days of files the folder keeps.
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
pub fn init(level: &str, log_dir: Option<&Path>, log_days: u32, console: bool) -> Result<()> {
    let verbose = Arc::new(AtomicBool::new(verbose_for(level)));
    let from_env = std::env::var_os("RUST_LOG").is_some();
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(directives(level)));
    let (filter, handle) = reload::Layer::new(filter);

    let file_layer = match log_dir {
        Some(dir) => {
            // Written synchronously, on purpose: the appender itself is the
            // writer, not `tracing-appender`'s non-blocking worker. Buffering
            // on a background thread would save nothing at this volume and
            // costs the only lines that really matter: the release profile
            // aborts on panic, so nothing is dropped and a buffered crash
            // report is never flushed.
            //
            // Two processes write the same file at once when `stop` or the
            // installer asks a running watcher to quit. The appender opens
            // it in append mode, FILE_APPEND_DATA and not FILE_WRITE_DATA, so
            // Windows places every WriteFile at the end of the file itself,
            // and the formatter hands a whole line to one write: lines
            // interleave, never tear. Measured on 2026-09-18 with 80
            // processes writing at once, and again through the appender in
            // the spike of 2026-09-25.
            Some(
                fmt::layer()
                    .event_format(Line {
                        verbose: Arc::clone(&verbose),
                        ansi: false,
                    })
                    .with_writer(appender(dir, log_days, Rotation::DAILY)?),
            )
        }
        None => None,
    };

    // Colours only when a person is really at a terminal. Unconditional ANSI
    // writes escape codes into whatever captures the output -- `run > log.txt`,
    // or a pipe -- where they are noise rather than colour.
    let console_layer = console.then(|| {
        fmt::layer().event_format(Line {
            verbose: Arc::clone(&verbose),
            ansi: std::io::stdout().is_terminal(),
        })
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .init();
    // Set once; a second `init` would have failed just above.
    let _ = LIVE.set(Live {
        filter: handle,
        verbose,
        from_env,
    });

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
                verbose: Arc::new(AtomicBool::new(verbose)),
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

    /// The timestamp as it has always read: local date and time, to the
    /// millisecond, the day the same as `local_now`'s.
    #[test]
    fn a_line_starts_with_the_local_time_to_the_millisecond() {
        let line = render(false, || {
            tracing::info!(target: target::WATCHER, "x");
        });
        let stamp = &line[..23];
        let shape: String = stamp
            .chars()
            .map(|c| if c.is_ascii_digit() { '0' } else { c })
            .collect();
        assert_eq!(shape, "0000-00-00 00:00:00.000", "{line}");
        assert_eq!(&stamp[..10], &local_now()[..10], "{line}");
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

    /// A scratch folder of its own for each test that touches files.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gamemode-executor-logging-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_log_files_are_ours_by_prefix_and_suffix() {
        assert!(is_log_file("gamemode-executor.2026-09-25.log"));
        assert!(
            is_log_file("gamemode-executor.log"),
            "the file before rotation"
        );
        assert!(!is_log_file("gamemode-executor.2026-09-25.txt"));
        assert!(!is_log_file("notes.log"));
    }

    #[test]
    fn the_file_being_written_is_the_log_file_modified_last() {
        let dir = scratch("current");
        assert_eq!(to_open(&dir), dir, "no log file yet: the folder");
        let now = std::time::SystemTime::now();
        for (name, age) in [
            ("gamemode-executor.2026-09-24.log", 60),
            ("gamemode-executor.2026-09-25.log", 10),
            ("gamemode-executor.log", 3600),
            ("notes.txt", 0),
        ] {
            let file = std::fs::File::create(dir.join(name)).unwrap();
            file.set_modified(now - std::time::Duration::from_secs(age))
                .unwrap();
        }
        assert_eq!(
            current_log(&dir),
            Some(dir.join("gamemode-executor.2026-09-25.log"))
        );
        assert_eq!(to_open(&dir), dir.join("gamemode-executor.2026-09-25.log"));
        let mut all = log_files(&dir);
        all.sort();
        assert_eq!(all.len(), 3, "{all:?}");
    }

    /// The appender as the file layer builds it: the oldest files pruned to
    /// make `log_days` or one more, the line in the file it opens, and that
    /// file the one *Open log* finds.
    #[test]
    fn the_appender_keeps_log_days_and_writes_where_open_log_looks() {
        let dir = scratch("appender");
        // Created one after the other, so their creation times, which the
        // library sorts by, are in this order.
        let old = [
            "gamemode-executor.log",
            "gamemode-executor.2000-01-01.log",
            "gamemode-executor.2000-01-02.log",
            "gamemode-executor.2000-01-03.log",
        ];
        for name in old {
            std::fs::write(dir.join(name), "planted\n").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let layer = fmt::layer()
            .event_format(Line {
                verbose: Arc::new(AtomicBool::new(false)),
                ansi: false,
            })
            .with_writer(appender(&dir, 2, Rotation::DAILY).unwrap());
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: target::WATCHER, "written through the appender");
        });

        let mut kept: Vec<String> = log_files(&dir)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        kept.sort();
        assert_eq!(
            kept.len(),
            3,
            "two days, today's included, and one more: {kept:?}"
        );
        assert!(kept.contains(&old[2].to_owned()) && kept.contains(&old[3].to_owned()));
        let written = current_log(&dir).unwrap();
        assert!(
            std::fs::read_to_string(&written)
                .unwrap()
                .contains("written through the appender"),
            "{}",
            written.display()
        );
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
