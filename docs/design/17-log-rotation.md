# Lot 17 — A log that turns over each day

**Status: done 2026-09-26.** Proposed 2026-09-24 by the maintainer, and
decided the same day: `tracing-appender`, with its constraints. Taken on
2026-09-25, after Lots 8 and 9, on the condition that a spike show the file
turning over without a restart — it does, below. Closed with three things
left to see, none a reason to reopen it: the updater's zip path at the next
release, the old single file pruned around 2026-10-01, and the next real
`purge` taking the updater's folder.

- [x] A spike, reproducible, of the turnover inside a running process and of what the library prunes, and when — 2026-09-25, below
- [x] The log written through `tracing-appender`'s `RollingFileAppender`, daily, synchronously, with every line as it is today — built 2026-09-25, below
- [x] `log_days` in `config.toml`: optional, 7 when absent, a whole number of 1 or more, validated like every other key — built 2026-09-25
- [x] What the library now does removed from our code, and nothing kept that it already answers — 2026-09-25, below, the local timestamps included
- [x] *Open log*, `purge` and the documentation following the dated files — built 2026-09-25
- [ ] The commands that write the same log — `stop`, `init`, `install-task`, `uninstall-task`, `update` — lose no line and prune nothing they should not — the package's install and uninstall, `purge` and `update --check` seen 2026-09-26, below; the updater's zip path waits for the next release
- [x] Verified in the field across a real turnover on the maintainer's machine — 2026-09-26, below

**Done when** each day's log is its own file, `gamemode-executor.YYYY-MM-DD.log`,
the last `log_days` of them are kept, every line reads as it does today in
the file and in a console, *Open log* opens the file being written, and
the program carries no code of its own for what the library does.

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

## The spike, 2026-09-25

The condition the maintainer set: if the program had to restart at
midnight to write in the next file, `tracing` would be ruled out. The
ideal stays a file that always has the same name and is renamed at the
turnover; `tracing-appender` cannot give it, which was accepted on
2026-09-24.

**Read first, in `tracing-appender` 0.2.5's `rolling.rs`.** Every write
asks whether the next turnover time has passed. If it has, the appender
prunes, opens the next file, and writes there. That is `should_rollover`,
called from `write` and from `make_writer`. The rotations differ only in
`next_date` and `round_date`: a minute, an hour or a day added, then
rounded down. So a minutely appender runs the daily one's code with a
shorter period. That is what makes the spike reproducible without
touching the machine's clock. The pruning, `prune_old_logs`, runs when
an appender is built, and at each turnover. It counts every file whose
name starts with the prefix and ends with the suffix. It sorts them by
creation time and deletes the oldest until `max_log_files - 1` are
left, to make room for the file about to be opened.

**Run** by `rollspike`, a scratch program, and a PowerShell script. The
script makes a fresh folder and plants the files a first start would
find: today's `gamemode-executor.log`, created three days ago; two older
rolled files; `notes.txt`; and `gamemode-executor-extra.txt`, our prefix
with another suffix. It then starts, at second 40 of a UTC minute, a
writer that logs a line a second for 95 s, as the watcher would.
Two short writers log three lines each and exit, as `stop` would: one at
second 50, before the boundary, and one at second 10 of the next minute.
`max_log_files = 3`.

| When, UTC | What happened | Our files, after |
| --- | --- | --- |
| 17:44:40 | the long writer built its appender | two old rolled files and the 17:44 file; **`gamemode-executor.log` deleted**, the oldest |
| 17:44:50 | a short writer, same minute | one old rolled file and the 17:44 file: one old file deleted at its build, then it appended to the 17:44 file |
| 17:45:00.32 | the long writer's line 21, first after the boundary | the 17:45 file opened by the same process |
| 17:45:10 | a short writer, new minute | the 17:44 and 17:45 files: the last old file deleted at its build |
| 17:46:00.35 | the long writer's line 81 | the 17:46 file opened, three files, nothing to prune |

- **No restart.** The same process, pid 2032, wrote lines 1–20 in the
  first file, 21–80 in the second, 81–95 in the third. Each turnover
  happened at the first line after the boundary.
- **Several processes, one file.** The long writer and the short ones
  appended to the same file. All 104 lines were whole and in order.
- `notes.txt` and `gamemode-executor-extra.txt` were left alone.
- **Every process that builds an appender prunes.** The short writers
  each deleted a file. Built on a file that already exists, the appender
  prunes to `max_log_files - 1` and opens that same file, so the count
  after a start is one below the count after a turnover. With
  `max_log_files = log_days + 1`, the folder holds `log_days` or
  `log_days + 1` files, never fewer. That is the mapping to use:
  `log_days = 7` keeps the last seven days the program wrote in, today
  included, and sometimes one more.
- **Today's `gamemode-executor.log` counts as one of the kept files** and
  goes when it is the oldest. Nothing is needed to clear it.
- A file the library cannot delete, or a delete two processes race for,
  is reported on standard error by the library itself. The console binary
  would print that in the terminal of whoever typed the command. That is
  rare and harmless, and the library does not let us route it.

**What the spike does not show:** a real daily turnover at midnight UTC.
The code path is the one measured; the field run closes it.

## The field run, 2026-09-25 and 26

The branch's build went onto the maintainer's installed copy at 20:04 on
the 25th, the old single file beside it. The first `install-task` line and
the watcher's start went into `gamemode-executor.2026-09-25.log`, every
line as before; the `stop` just ahead of them, from the executable being
replaced, into the old file, as it should.

**The turnover, measured.** The maintainer logged off at 00:12:43 local
and on again at 00:13:29; the watcher that started then, pid 19944, wrote
into the file of the 25th, UTC being 22:13. It then wrote nothing while
the machine sat overnight, and at 15:11:58 on the 26th the maintainer
chose *Open log*. The menu opened the file of the 25th — the last one
written, today's not existing yet — and the helper's *Opened* line, the
first line after midnight UTC, made the same process open
`gamemode-executor.2026-09-26.log` and write there. A second click, at
15:12:09, opened the new file. No restart; the maintainer confirmed both
openings as described beforehand.

**The setup commands' three callers.** By hand, `stop` and `install-task`
on each build that went onto the installed copy: their lines where the
watcher's were. The package, on 2026-09-26: the maintainer's purge ran its
uninstall, whose `stop` and `uninstall-task` wrote and were swept (below);
then the package from `dist\` was installed at 15:42:13, and `init`'s
*Configuration kept*, `install-task`'s *Logon task registered* and the new
watcher's start went into the file of the 26th, the three files there left
alone, far from the eight `log_days = 7` keeps. `purge` itself writes no
log. `gamemode-executor update --check`, typed by the maintainer at
15:52:59: its four lines in the file of the 26th, as in the terminal.

The updater runs the package for an installed copy, the path above. Its
zip path cannot be seen yet, for a reason worth writing down: the shell it
leaves expands the new release and runs `install-task` from the files it
has just put in place, so the command that writes is the release's, not
the one updating. Updating a zip copy now could only install 0.3.0, whose
`install-task` predates this lot. It is seen at the next release: a zip
copy of 0.3.0 updated to the release that carries this lot.

The footprint, the watcher just started: 161 handles and 1.99 MB private,
against 162 and 2.09 MB for 0.3.0 just started in
[Lot 16](16-footprint.md)'s table. The executables grew by 90 KB, the
appender and the `time` crate.

## What changes for the person reading the log

- The day turns at midnight UTC: 02:00 in French summer, 01:00 in winter.
  A session at 01:30 on the 24th, summer time, is in the file dated the
  23rd. Every line keeps its local timestamp.
- The file being written is dated too: `gamemode-executor.2026-09-24.log`
  — prefix `gamemode-executor`, suffix `log`, so it still opens in a text
  editor on a double-click. There is no `gamemode-executor.log` any more.
- `log_days` keeps that many *files*: the last seven days the program
  wrote a line, not the last seven calendar days — and sometimes one more,
  as the spike shows.
- The first files are dated in the new style; the old
  `gamemode-executor.log` matches the prefix and the suffix, and the
  library counts it as one of the kept files and deletes it in its turn —
  measured in the spike.

## Our code, against what the libraries do

Gone, 2026-09-25:

- The file opened by hand into a `Mutex`, and the comment that explained
  why the log did not rotate.
- `LOG_FILE_NAME`, and the one-file assumption in `purge`, in the tray's
  targets and in `service`. `logging` now says what a log file is, by the
  library's own rule, prefix and suffix, and which one is being written.
- `LocalTimestamp` and its `unsafe` call to `GetLocalTime`: the timestamp
  is `tracing-subscriber`'s `LocalTime`, in the same format. The caveat this
  page recorded is Unix's. On Windows the `time` crate asks
  `SystemTimeToTzSpecificLocalTime`, thread-safe by its own comment, and a
  scratch program read `+02:00` from a second thread with no configuration
  flag. `local_now`, the marker's time, goes through the same crate.
- The four copies of "the configuration's `log_dir`, or `logs` in the local
  folder", now `General::log_dir` and `config::default_log_dir`, and the
  watcher's own default level, now the configuration's default.

Stays, each for a reason written in its module:

- `Line`, the formatter: the two readers of `logging`'s module comment.
  `tracing-subscriber`'s formats always print the fields; ours prints them
  only at `debug`.
- The panic hook, the category filter and the live level: `tracing` has
  none of the first, and the other two are already
  `tracing-subscriber`'s own `EnvFilter` and `reload`.

**As built.** `logging::appender` builds the appender every process uses,
daily, `max_log_files = log_days + 1` for the reason the spike gives, and
the file layer writes through it directly. `presence-probe log-turnover`
builds the same appender turning over each minute and replays the spike
in a scratch folder; run on 2026-09-25 at 19:56, it gave the table above
line for line. Tests: `log_days` refused at 0, a decimal, a negative
number or text, and accepted from 1; the file being written found among
dated, old and foreign files; an appender built with `log_days = 2` over
four old files keeping two of them and writing its line where *Open log*
looks; the timestamp's format pinned. The new dependencies are
`tracing-appender`, and `time` directly for the timestamp's format.

## Decided 2026-09-25

- *Open log* opens the file being written, and the folder only if finding
  that file proves unreliable — the maintainer. `RollingFileAppender` does
  not say which file it writes: `join_date` is private, confirmed in its
  code. The file being written is the most recently modified one bearing
  the prefix and the suffix, since the library opens a new file only to
  write a line in it. That rule needs neither the library's date format
  nor its clock, only the prefix and suffix this program chooses.
- `log_days` is optional and 7 when absent; its absence never turns the
  rotation off. A whole number of 1 or more, 1 keeping today's file
  alone; a decimal, 0 or a negative number is a fault like any other —
  the maintainer.
- `log_days` applies at the next start, as `log_dir` does, said at `warn`
  on a reload that changes it: the appender is built once, and rebuilding
  it live would take the whole file layer through `reload` for a key
  nobody changes twice.
- `purge` removes every log file, dated or not.
