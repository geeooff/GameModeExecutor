# Lot 17 — A log that turns over each day

**Status: proposed 2026-09-24**, by the maintainer. Decided the same day:
`tracing-appender`, with its constraints. To be weighed against
[Lot 18](18-game-gone-nobody-there.md) once the open points of Lots 8 and 9
are done — the order set on 2026-09-25.

- [ ] The log written through `tracing-appender`'s `RollingFileAppender`, daily, synchronously, with every line as it is today
- [ ] `log_days` in `config.toml`, 7 when absent, validated like every other key
- [ ] What the library now does removed from our code, and nothing kept that it already answers
- [ ] *Open log*, `purge` and the documentation following the dated files
- [ ] The commands that write the same log — `stop`, `init`, `install-task`, `uninstall-task`, `update` — lose no line and prune nothing they should not
- [ ] Verified in the field across a real turnover on the maintainer's machine

**Done when** each day's log is its own file, `gamemode-executor.YYYY-MM-DD.log`,
no more than `log_days` of them are kept, every line reads as it does
today in the file and in a console, and the program carries no code of
its own for what the library does.

## What was asked, and what was decided

The maintainer, 2026-09-24: the log turns over every day, on local time;
some days of history are kept, the number in `config.toml`, 7 when the key
is absent; the current file keeps its name, the history takes the date.
`RollingFileAppender` was named as possibly simple.

Then, after the libraries were compared: a mature library the Rust
community agrees on, even at the price of its constraints, over code of
our own; our features kept — the file, the console in colour, the level
changed live; and any code of ours that the library makes redundant
removed.

## The libraries compared, 2026-09-24

| | Local time | Today's file keeps its name | History dated | Keeps our `tracing` chain | Downloads, recent |
| --- | --- | --- | --- | --- | --- |
| `tracing-appender` 0.2.5 | no, UTC | no, dated | yes | yes | 32.5 M |
| `log4rs` 1.4.0, log4j's port | yes | yes | no, numbered | no, `log` facade | 1.9 M |
| `file-rotate` 0.8.0 | yes | yes, but no dot in the name | yes | yes | 1.07 M |
| `rolling-file` 0.2.0 | not checked: no release since January 2023 | | | | 1.1 M |

- `tracing-appender` belongs to the `tracing` project this program
  already uses. Its documentation: it "will automatically append the
  current date and hour (UTC format) to the file name"; local time has
  been asked for since 2021
  ([#1645](https://github.com/tokio-rs/tracing/issues/1645),
  [#2691](https://github.com/tokio-rs/tracing/issues/2691)) and not added.
  `latest_symlink` would give today's file a fixed name, but Windows makes
  a symbolic link for a process that is not elevated only in *Developer
  Mode* ([CreateSymbolicLinkW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createsymboliclinkw)).
  `max_log_files` keeps a number of files, not of days.
- `log4rs` rolls on local time and keeps the current name, but numbers its
  history, and is built on the `log` facade: moving to it means bridging
  every event, and rewriting this program's line format as its encoder.
- `file-rotate` does all three on paper, but its documentation forbids a
  dot in the base file name, its code assumes one process per file, and it
  dates the history *yesterday* whatever day the file holds.
- No port of NLog exists; the crate named `nlog` writes into Notepad.

## What changes for the person reading the log

- The day turns at midnight UTC: 02:00 in French summer, 01:00 in winter.
  A session at 01:30 on the 24th, summer time, is in the file dated the
  23rd. Every line keeps its local timestamp.
- The file being written is dated too: `gamemode-executor.2026-09-24.log`
  — prefix `gamemode-executor`, suffix `log`, so it still opens in a text
  editor on a double-click. There is no `gamemode-executor.log` any more.
- `log_days` keeps that many *files*: the last seven days the program
  wrote a line, not the last seven calendar days.
- The first files are dated in the new style; the old
  `gamemode-executor.log` matches the prefix and the suffix, and to be
  checked is whether the library counts it as one of the kept files and
  deletes it in its turn.

## Our code, against what the libraries do

To go:

- The file opened by hand into a `Mutex`, and the comment that explained
  why the log did not rotate.
- `LOG_FILE_NAME`, and the one-file assumption in `purge`, in the tray's
  targets and in `service`.

To stay, each for a reason written in its module:

- `Line`, the formatter: the two readers of `logging`'s module comment.
  `tracing-subscriber`'s formats always print the fields; ours prints them
  only at `debug`.
- `LocalTimestamp` and `local_now`, through `GetLocalTime`:
  `tracing-subscriber`'s `LocalTime` needs the `time` crate's local offset,
  which its documentation ties to an `unsound_local_offset` configuration,
  and `ChronoLocal` would bring `chrono` in for ten lines. To be checked on
  Windows before it is decided for good.
- The panic hook, the category filter and the live level: `tracing` has
  none of the first, and the other two are already
  `tracing-subscriber`'s own `EnvFilter` and `reload`.

## Open, for the maintainer

- *Open log*: `RollingFileAppender` does not say which file it is writing
  — to be confirmed in its code. Opening the log folder instead reads the
  same in Explorer and needs no copy of the library's naming rule; a menu
  that works out today's name itself is the kind of duplicate this lot is
  asked to remove.
- `log_days` applied at the next start, as `log_dir` is, since the
  appender is built once; or rebuilt live through the same `reload` the
  level uses.
