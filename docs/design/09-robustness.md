# Lot 9 — Robustness

**Status: partly done.** The session marker is built and verified; the
configuration-fault design is decided and waiting; two smaller items remain.

- [x] Restore at the next start what a logoff could not — done 2026-09-16, a race fixed and re-verified 2026-09-17
- [ ] Configuration faults shown in the tray, and live reload — designed, below
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

**Mechanics.** `serve` loads the configuration itself and takes the path rather
than a `Config`; the engine reads an `Arc<RwLock<Config>>` at each use, so a
swap needs no wake-up. The tray gains a fault overlay on top of the session —
two different axes — and finally sets `State::Error`. Written so Lot 12 is
small: the watcher takes a path and an "apply" action.

## Stop timing the refinement

The single attempt at `identify_after` is a lottery with three ways to lose:
the process being named is already dead and one candidate is left (now
handled by the survivor rule), the game is still loading so nothing renders,
or the counters cannot be read. Each bail-out spends the one attempt. Two
Battlefield 6 sessions an hour apart lost it and won it: the second read 0.0 %
for every candidate at T+10 s and 75 % at T+20 s. A margin, not a calibration.

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
