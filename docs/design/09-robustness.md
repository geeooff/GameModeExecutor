# Lot 9 — Robustness

**Status: partly done.** The session marker is built and verified; the
configuration faults and the live reload are built and verified in the
field; two smaller items remain.

- [x] Restore at the next start what a logoff could not — done 2026-09-16, a race fixed and re-verified 2026-09-17
- [x] Configuration faults shown in the tray, and live reload — built 2026-09-19, measured without a game and then verified across two Starfield sessions on 2026-09-20, below
- [ ] Stop timing the refinement; let the OS say when — below
- [ ] `ShutdownBlockReasonCreate`, so Windows' shutdown screen says what is being restored rather than naming the process
- [ ] Behaviour across two games launched back to back
- [x] Give the engine a seam, so its loop can be tested without a game — done 2026-09-17, below

## The session marker

A process started even one millisecond after `WM_QUERYENDSESSION` dies with
`STATUS_DLL_INIT_FAILED` — measured in [Lot 5](05-windowless-watcher.md) — so
the stop commands cannot run at session end. The mechanism that does not
depend on Windows' timing:

- `fire_start` writes `pending-stop-actions` — no extension — at the root of
  `%LOCALAPPDATA%\GameModeExecutor`, naming the game and the time; the
  refinement rewrites it with the better name; a game that stops on its own
  removes it after the commands ran, best effort as before.
- When the watcher is stopped mid-game the commands are still attempted, and
  the marker is removed only when they can be **confirmed**: at least one
  command was waited for, and every waited-for command exited 0. Otherwise it
  stays, with a warning saying the next start will retry. A fire-and-forget
  command never confirms anything — deliberately conservative, because the
  failure this exists for is a process created and dying unseen, which is
  exactly what a fire-and-forget command cannot report.
- The writer's exit and the watcher's stop can arrive **together**, and
  then the same rule applies, whichever came first. A game that stopped on
  its own and a stop signalled during the grace period look alike from the
  loop; a stop that is set once the grace is over makes it the mid-game case.
- `stop_actions_on_exit = false` removes the marker on exit instead, so the
  opt-out is not undone at logon. A crash never reaches that branch, so crash
  recovery does not depend on the setting.
- At start, before watching, a marker present means the last session never
  closed: one `info` line naming the game, the stop commands, the marker
  removed. Logoff, shutdown, crash and power cut are one case.
- **Since 2026-09-18, the writer is looked for first.** A marker present
  with the presence writer still running means the game never ended: the
  last watcher handed the session over — `stop --handover`, which an update
  or an upgrade uses because a watcher follows within the second — or died
  under it. Then nothing runs, neither stop nor start, and the session is
  resumed from the marker: name, icon, the wait on the writer's handle. The
  stop commands run at the end of the game as they always did. Without this
  an update mid-game switched the configuration off and on again two
  seconds apart. Three scenarios in `engine/tests.rs`; the design is in
  [Lot 13](13-updating.md).

**Where it lives, and why not in `logs\`.** A logs folder is disposable by
nature and gets emptied without a second thought, which would take a pending
recovery with it; state does not belong among files anyone is entitled to
throw away. Not next to the configuration either: that may sit in `%APPDATA%`,
which roams, and a marker following the profile to another machine would run
the stop commands there. The registry was considered — the most Windows-native
home for a value this small — and declined for the footprint: it would split
the program across two places and leave residue that deleting the folder does
not remove. No extension on purpose: `.txt` says "a note for a person" and is
the first thing a tidy-up deletes; a bare name says "the program's business".
`status` reports the marker and its path, and tells an open session from a
stale one by whether the presence writer is running.

Verified in the field the same night: `status` showed the marker present
during a game and `none` after a normal stop; the recovery path was verified
with a marker planted by hand.

**The race, measured 2026-09-17.** A second real logoff with Skyrim running
found the case the rule above did not cover. On the 16th the stop signal had
reached the loop first and the mid-game branch kept the marker. This time
Windows killed the presence writer **5 ms after** asking the session to end,
`WaitForMultipleObjects` reported the writer's handle -- it sits before the
stop event in the array, and wins when both are signalled -- and the loop took
the ordinary path: the command failed with `STATUS_DLL_INIT_FAILED`, said so
at `warn`, and the marker was removed regardless. Nothing ran at the next
logon and the fans stayed on the gaming configuration.

```
09:32:20.337  Windows asked to end the session, so the watcher starts stopping now
09:32:20.342  Windows released the presence writer while the identified game is still running
09:32:20.343  Game no longer detected: SkyrimSE.exe
09:32:20.447  WARN `FanControl - Idle` failed  status=exit code: 0xc0000142
09:33:20.428  GameModeExecutor 0.1.0 (86d8d1ac) starting        ← no recovery line
```

The fix is one condition in the loop: after the grace period, a stop that is
set makes the exit the mid-game case. `engine/tests.rs` scripts the race --
the sensor signals the stop as it reports the writer's exit -- and the
scenario failed on the old loop before it passed on the new one. The same
morning's three ordinary sessions -- Skyrim, Battlefield 6 with its rename,
and a *Quit* mid-game whose commands were confirmed and marker removed -- all
behaved as documented.

The logoff was repeated on the fixed build twenty minutes later and the whole
chain held: `could not be confirmed` at `warn` before the session ended, the
recovery line 44 s later at logon, the command exiting 0. That run happened
to deliver the stop first, as on the 16th; the writer-first ordering has been
seen once in the field and is what the scenario pins down. Which order a
logoff produces is Windows' choice, and both now lead to the same branch.

## Configuration faults, shown where the program already lives

**The problem.** A configuration the windowless watcher cannot read fails with
no trace. `Config::load` runs before `serve` initialises the log, so the
`tracing::error!` in `main` fires with no subscriber. A typo, an old format or
an empty file at logon means exit code 4, no log line, no window, and a fan
profile that never changes again with nothing to say why.

**Not a `MessageBox`.** A program built to be discreet does not put a dialog on
the screen at logon. It has a place on screen already — the icon — and an
error state nobody has used, reserved in [Lot 6](06-notification-icon.md).

**The behaviour:**

- **Start regardless.** The watcher starts even when the file is unreadable:
  log at the default location, window, icon in the error state, tooltip
  `GameModeExecutor - configuration error`, and the disabled first menu entry
  saying what is wrong in one line — `line 3: unknown field 'log_levl'`. *Edit
  configuration* keeps working, because it is the fix. Nothing is watched until
  a valid configuration exists, and a pending session marker is honoured the
  moment one does.
- **Watch the file.** `FindFirstChangeNotificationW` on the configuration's
  folder, on a small thread that waits on that handle and the stop event and
  posts a message to the window. Debounced — editors write in several steps —
  so the reparse waits ~250 ms after the last notification. This is also the
  live reload: save the file, and the change applies.
- **Valid again:** applied, icon back to idle or active, `info`
  `Configuration reloaded`. **Invalid: the program is disabled outright** —
  decided 2026-09-16, never a fallback to the last valid configuration.
- **What applies live:** the commands, `[detection]`, `stop_actions_on_exit`.
  `log_dir` waits for the next start, said at `warn`. `log_level` can follow
  live through a `tracing_subscriber::reload` layer if that stays a few lines.

**Why disable outright.** "Disabled" means *frozen*: nothing runs, not the old
commands and not their stop half, so a game in progress keeps its profile and
the session marker stays where it is. When the file is valid again, the marker
mechanism finishes the job by itself — game gone in the meantime, recovery
runs the stop commands; game still running, the engine detects it afresh. One
detail to get right: look for the writer *before* recovering, or a game still
on would get the idle then the gaming configuration a few milliseconds apart.
And the strict rule is what [Lot 12](12-editing-on-a-copy.md) earns: once
editing goes through a staged copy, the only way to put an invalid file on
disk is to edit it by hand outside the program, and then a frozen program with
a red icon is the honest answer.

**Mechanics, as planned.** `serve` loads the configuration itself and takes
the path rather than a `Config`; the engine reads an `Arc<RwLock<Config>>`
at each use, so a swap needs no wake-up. The tray gains a fault overlay on
top of the session — two different axes — and finally sets `State::Error`.
Written so Lot 12 is small: the watcher takes a path and an "apply" action.

**Mechanics, as built — 2026-09-19.** The `RwLock` was not built. Reading
the configuration at each use would have covered a *valid* change and left
the *invalid* one to new engine states: frozen while idle, frozen mid-game
with the writer's exit meaning nothing, then a recovery to re-run once the
file is valid again — each a branch in the loop and a scenario nobody had
written. The handover from [Lot 13](13-updating.md) already had every one of
those: an engine that stops with the session left open in the marker, and a
start that looks for the writer before recovering. So a change to the file
is a **handover from one engine to the next in the same process**:

- `service` runs a supervisor on the worker thread — one engine per usable
  configuration, built on the file as it is. The engine is unchanged but
  for one line: a stop whose reason is `Reload` returns the way `Handover`
  does, marker kept, nothing run.
- `StopSignal` gained a third reason and a **child**: a signal that is set
  when either its own event or its parent's is, with the parent's reason
  winning. The engine runs on a child of the process-wide stop; the child's
  own event carries the reload and is reset between engines
  (`take_reload`); the parent carries *Quit*, the logoff and `stop`, and is
  never reset. That is what keeps a *Quit* arriving during a reload from
  being lost, without a lock around the wait.
- `config::watch` is the thread: `FindFirstChangeNotificationW` on the
  folder, waited on with the process stop, a 250 ms settle after the last
  notification because editors write in several steps, and a comparison of
  the file's *bytes* with what is running — a folder touched or a file
  written back unchanged is not a reload, a file that no longer parses is.
- A file that cannot be used is a `LoadError` with a one-line `summary`
  — `line 3: unknown field `log_levl`, expected one of …`, `the file is
  missing`, `detection.poll_interval must be greater than zero` — reported
  to the tray through a `FaultSink` beside the session sink. The tray reads
  both facts together: a fault is `State::Error` on every surface, the
  tooltip *configuration error*, the menu's first line `Configuration
  error: ` and the summary, cut at 160 characters. The supervisor then parks
  on the child signal: the next change or the process stop ends that, and
  nothing else.
- What a reload applies to the log itself: `log_level` follows live through
  a `reload::Layer` around the filter and an atomic for the fields, unless
  `--log-level` or `RUST_LOG` fixed it at start; `log_dir` cannot follow —
  the file is open — and is said at `warn` to wait for the next start.
- `Config::load` no longer makes the watcher exit: the command line runs
  `serve` on the path alone, and the exit codes 3 and 4 are the other
  commands'. The recovery that closes a session now reports *Idle* to the
  tray, since after a reload the icon may still show the session the last
  engine left open; a first start swallows it as a repeat.

**Measured 2026-09-19, 01:58–02:00**, a development build on a scratch
configuration while the installed watcher was stopped, with no game: a
misspelt key at `T`, the `ERROR` line and the icon refreshed to `Error` at
`T + 250 ms` on the nose, the settle; the file fixed with `log_level` moved
to `info`, *Configuration reloaded* and the fields gone from the lines that
followed; back to `debug`, *Log level changed* and the fields back; a
`poll_interval` of zero, the validation's own sentence in the menu line;
the file deleted, *the file is missing*; the file back with `log_dir`
moved, the reload and the `warn` that the log waits; then `stop`, *Stopped*.
Six changes, six reloads, one process, 54 seconds.

**Verified in the field 2026-09-20, 14:41–14:54**, by the maintainer on
the installed copy, with the release build of the branch copied over it:

- A misspelt key while idle: the `ERROR` line, the red icon, the tooltip
  and the menu line; fixed, *Configuration reloaded*, the icon grey. The
  maintainer's remark that the menu line is long — the parser's list of
  expected fields — is accepted as it is, for want of a better single line.
- **A reload during a game.** Starfield detected at 14:47:32, the start
  commands run; the stop action renamed in the file at 14:49:01: *Stopping
  for a reload*, *Configuration reloaded*, *A session was left open with
  Starfield.exe still running, so it resumes where it was* — no command
  run, no beep, the icon green throughout. The game quit at 14:49:40 and
  the stop commands that ran were the renamed ones: `FanControl - Idle
  (reloaded)`.
- **A fault during a game.** Starfield again at 14:51:45; `gpu_sample`
  misspelt at 14:52:43: the engine stopped, the red icon, and the game
  quit into a frozen watcher — nothing ran, the fans stayed on the gaming
  configuration, as the strict rule says. The file fixed at 14:54:39:
  *Configuration reloaded*, then *The last session ended with Starfield.exe
  still running and its stop commands never ran, so they run now*, and
  the idle configuration came back by itself. Between the two, the icon
  showed *playing Starfield.exe* for 6 ms — the session the stopped engine
  had left in the tray, until the recovery reported *Idle* — which is the
  reason that report exists.

**Said with a notification, decided 2026-09-20** on the maintainer's
remark after the run: the red icon is easy to miss at logon and the menu
line was too long to read. So a fault is said, silently, with the shell's
error glyph and the whole summary — at start and at every reload that
fails — and the end of a fault is said too, so the person knows the
watcher is back; a reload that stays usable says nothing. The menu line
keeps a headline, the summary cut before the parser's list of expected
fields. This is the second thing the program ever says unasked, beside the
answer to a click; the principle in `AGENTS.md` names both. The tray
decides the wording, the supervisor decides which transition it is — a
`Report` of *faulty*, *restored* or *usable* — since the notice depends on
what came before, which only the supervisor knows. What came before
includes the last process: the maintainer broke the file, stopped, fixed
it, started again and got no word, so a fault is noted in a second file
beside the session marker, `configuration-fault`, removed when a usable
file is read, and a start that removes one says the fault is over. `purge`
removes it with the rest.

Two defects the run found, both fixed the same day:

- `log_level = "debg"` was **not** a fault. `validate` did not look at the
  value, the filter took no directive from it and fell back to `error`
  alone, and the log went quiet from 14:42:33 to 14:44:07 — the *reloaded*
  line that should have said what happened was itself filtered out. A
  pre-existing hole, first seen because the reload made the file easy to
  break: the five levels are now validated, case-insensitively, and a
  sixth word is a fault with the icon and the menu line like any other.
- The debug line at the writer's exit, after a resume, read *the game was
  never named* with a session of 34 s: the resumed signal has a name and
  no process id, and the engine's clock started at the resume. It now says
  the game was known by name only, from the resumed session, and gives the
  time since the resume rather than a session length it cannot know.

## Stop timing the refinement

The single attempt at `identify_after` is a lottery with three ways to lose:
the process being named is already dead and one candidate is left (now
handled by the survivor rule), the game is still loading so nothing renders,
or the counters cannot be read. Each bail-out spends the one attempt. Two
Battlefield 6 sessions an hour apart lost it and won it: the second read 0.0 %
for every candidate at T+10 s and 75 % at T+20 s. A margin, not a calibration.

A third session, on the second machine on 2026-09-18 at 21:15, shows the
other side of the lottery. Windows' per-user list there carries `chrome.exe`
— the list grows with whatever the Game Bar was once used over, as far as
this record understands it — so the session was named after Chrome first,
and the one attempt at T+21 s read `Overwatch.exe` at 3 %, a loading
screen's worth, and renamed it. Enough that time; had the game rendered
nothing yet at T+21 s, the session would have stayed *chrome.exe* to the
end, in the log and in the placeholders.

Two changes worth weighing, in order of appetite:

- **Only count an attempt that reached a verdict.** "Nothing is rendering yet"
  and "no candidates" re-arm the timer instead of ending it, with a cap so a
  session that never settles cannot retry forever. Small, and it closes the
  loading-screen hole.
- **Wait on the named process instead.** The engine already parks on the
  presence writer's handle; parking on the *named* process's handle too, and
  re-identifying when it exits, needs no timer and no polling. The event that
  mattered — the launcher exiting — would have woken it exactly then. The same
  OS-native shape the rest of detection uses.

## The engine has no tests, and the reason is structural

Measured 2026-09-17 with `cargo llvm-cov`, ignored tests included: the
library sits at 55 % line coverage. The pure modules are where they should
be — `actions` 91 %, `marker` 94 %, `detect` 77–91 %, `exit` 100 % — and the
Win32 plumbing (`win`, `service`, most of `tray`) is at or near zero, which is
expected: it is verified by hand, and the design record says so with dates.

The number that is not acceptable is **`engine.rs` at 0 %**. The design
record calls the engine "the part worth keeping testable", and it is: the
session loop, the start and stop edges, the refinement's survivor rule, the
marker's confirm-or-keep decision on a mid-game stop, the recovery at start.
None of it runs under a test, because `Engine::new` reads the presence
writer's registration and `run` waits on real process handles.

The cut that fixes it is one trait for the I/O the engine performs, so the
logic stays in the engine and only the OS moves behind an interface:

```rust
pub trait Probe {
    fn writer_pid(&self) -> Option<u32>;
    fn wait_writer(&self, pid: u32, stop: &StopSignal, timeout: Option<Duration>) -> Result<WaitOutcome>;
    fn known_games(&self) -> Result<KnownGames>;
    fn snapshot(&self) -> Result<Snapshot>;
    fn rendering_load(&self, sample: Duration) -> Result<HashMap<u32, f64>>;
}
```

**Built the same day, as `sensor::Sensor`** — five methods, `Windows` as the
one real implementation, `Engine<S: Sensor>` generic so the dispatch is
static. The engine lost every OS call and 40 lines; `engine/tests.rs` drives
nineteen whole sessions through a scripted sensor with real `cmd.exe`
commands, a real marker file and a real stop event. `engine.rs` went from
0 % to 88 % line coverage, the library from 55 % to 64 %.

It also found a defect the field never had: a game Windows tracks but does
not name reached the tray as `None`, which was also the sink's word for "no
game", so the icon stayed grey through such a session. The sink now carries
a `Session` enum — `Idle` or `Playing(Option<GameSignal>)` — and the case
has a test. The manual `trigger` command no longer builds an engine at all;
it runs the commands, which is all it ever did.

**Measured again on 2026-09-18, with Lot 13's updater in:** the library at
70 % line coverage, ignored tests included, and `winhttp` at 92 % through a
listener the tests run themselves. The updater was written to the
same cut — the network behind `Feed`, the machine driven by events — and
sits at 76 % to 95 % per file, the shell scripts it generates checked for
their shape and the `pending`/`result` files exercised on scratch folders.
The command line gained parse tests and a machine-bound diagnostics test,
44 % from nothing. What stays near zero is what it should be: `service`,
`win`, and the parts of `tray` that are Win32 calls, verified by hand with
the dates in this record.
