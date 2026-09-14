# Development plan

Working document. Updated as work lands, not written once and forgotten.

Status: `[ ]` todo · `[~]` in progress · `[x]` done · `[-]` dropped, with a reason.

Lots 1 to 7 are **committed**, plus Lot 11, which is numbered last but was
written early — see the lot for why. Lots 8 to 10 are **proposed** and carry
only enough detail to decide whether they are wanted: writing detail is how
scope grows.

**Standing rule from Lot 11:** any lot that changes what the user sees updates
`docs/getting-started.md` and `docs/how-it-works.md` in the same commit.

The numbering has moved twice. On 2026-09-10 logging became Lot 4 and pushed
the rest up by one; on 2026-09-14 the tray icon lot was split in two — the
Windows-subsystem work became Lot 5, the icon itself Lot 6 — and pushed the
rest up again. Commit messages written before those dates use the numbers of
their day: "lot 4" in one of them means the tray icon, which is now Lot 6.

---

## Where we are

Detection works and is validated. The watcher observes the lifetime of the Game
Bar presence writer process, which is Windows' own verdict on whether a game is
running. Two real sessions confirmed it (Starfield on Game Pass, Farming
Simulator 25 on Steam), each producing exactly one start/stop pair, with alt-tabs
not disturbing it. See the README for the measurements and for why every other
mechanism was ruled out.

Some work already done lands in later lots than the order it was built in. That
is recorded per lot rather than hidden, so the plan reflects reality.

**Dependency order:** 1 → 2 → 4 → 5 → 6, with 3 independent, and 7 needing
both 3 and 6. Lot 4 sits before the icon deliberately: the icon will log too,
and writing it in the finished vocabulary is cheaper than converting it after.
Lot 5 sits before it for the same reason in the other direction: the icon hangs
off a window and a message loop, and those are worth proving with nothing on
screen before anything is drawn on them.

---

## Lot 1 — Correct console behaviour · committed · `[x]` done

Goal: the technical core, as a console program, correct and boring. Configured
commands run when they should and nothing else happens. Simple file logging that
says when a game is detected and when it is no longer.

- [x] Detection by presence writer lifetime: idle poll, then park on the handle
- [x] Action runner: args, working dir, env, no-window, wait, timeout
- [x] `stop_delay` honoured to the configured value (was rounding up to whole polls)
- [x] Configuration found without `--config`, `validate` / `init`
- [x] Per-user logon task via `schtasks`, no elevation
- [x] Single instance per session
- [x] Pipeline proven end to end with placeholder actions
- [x] File logging on by default, not only when `log_dir` is set
- [x] Log lines that state plainly: game detected, game no longer detected
- [x] Real FanControl actions wired, through a scheduled task (see below)
- [x] Register the two elevated tasks (needs one elevated prompt)
- [x] Whole chain proven against the real FanControl
- [x] Logon task installed, running the watcher hidden
- [x] Real game sessions: both games detected, named and switching profiles
- [x] Packaged title named after the fix, on both edges
- [x] Confirm the logon task across a reboot
- [x] Measure what happens between closing a game and the writer being released
- [-] Act on the game process exiting as well as the writer — dropped, see below
- [x] Log timestamps in local time rather than UTC
- [x] Logon task registered from XML, without the schtasks defaults that kill it

Done when: a real game session drives the configured commands, start and stop,
started automatically at logon, with a log file that shows what happened and no
known defects.

State:

- FanControl **v275, portable**, at
  `C:\Users\geoff\OneDrive\Applications\FanControl`, with both profiles now
  present: `Game.json` and `Quiet.json`.
- The `-c` CLI is correct, but **FanControl cannot be launched by this watcher**.
  Its manifest declares `requestedExecutionLevel level="requireAdministrator"`,
  so `CreateProcess` from an unelevated parent fails with error 740,
  `ERROR_ELEVATION_REQUIRED`. Found by running it for real, not by reading about
  it.
- The bridge is one scheduled task per profile, registered with *run with highest
  privileges*, triggered by the watcher with `schtasks /Run`. Triggering needs no
  elevation and raises no UAC prompt.
- Working config and the shipped example now use that form. The README gained a
  "Programs that require elevation" section, and its headline example no longer
  shows a FanControl command line that could never have worked.
- **Chain verified end to end 2026-09-09**, with the profile change read back
  from FanControl's own `CACHE` file rather than assumed:

  | Moment | Event |
  | --- | --- |
  | 23:49:52.754 | presence writer appears |
  | 23:49:53.680 | GAME DETECTED, task triggered, `schtasks` returns 0 |
  | 23:49:55.123 | FanControl is on `Game.json` |
  | 23:49:57.762 | presence writer exits |
  | 23:50:02.772 | GAME NO LONGER DETECTED after the 5 s grace, task triggered |
  | 23:50:04.349 | FanControl is back on `Quiet.json` |

  So about 2.4 s from game start to the profile being applied, and about 6.6 s
  from game end to it being restored, of which 5 s is the configured
  `stop_delay`. FanControl kept the same PID throughout: nothing started a
  second instance.

### Where the delay after quitting a game goes

Measured 2026-09-10 on Starfield, with `presence-probe watch` tracking both the
writer and the game process, and the user noting the exact moment they quit:

| Segment | Duration |
| --- | --- |
| click quit to the game process being gone | 2.8 s |
| **game process gone to Windows releasing the writer** | **52.2 s** |
| writer released to our stop actions | 1.9 s (`stop_delay`) |
| our action to the profile applied | about 1.4 s |

So roughly a minute from quitting to the fans changing, and **all but six
seconds of it is Windows**. Tuning `stop_delay` is noise at this scale.

Two more titles, measured the same way on 2026-09-10, settle where that 52 s
comes from:

| Title | Store | Game process gone to writer released |
| --- | --- | --- |
| Starfield | Game Pass | **52.2 s** |
| Forza Horizon 6 | Game Pass | 6.2 s |
| Battlefield 6 | Steam | 4.4 s |
| Battlefield 6, second session | Steam | **about 2 min 2 s** |
| Starfield, second session | Game Pass | **2.7 s** |

Both explanations offered earlier are dead: the game closing slowly (its
process was gone in 2.8 s) and cloud save synchronisation as a store-wide
behaviour.

**The last two rows kill the per-title theory outright.** The first three were
read as "Starfield is the outlier", one number per title. Then both titles were
measured a second time on 2026-09-10, and both moved — in opposite directions.
Battlefield 6 took **2 min 2 s** where it had taken 4.4 s. Starfield, the
supposed outlier at 52.2 s, released the writer in **2.7 s**. Same titles, same
stores, same machine, same afternoon.

So the delay is not a property of the title. No table of titles will ever
predict it, there is nothing to tune, and a session that stops promptly proves
nothing about the next one. Windows releases the writer when it decides to.

Measuring the second session of each needed a different source per store, since
our own log did not record the writer's exit until this was written: Steam's
`gameoverlay_ui.txt` for Battlefield 6, and for Starfield the AppX container
destruction in `Microsoft-Windows-AppModel-Runtime/Admin` (event 217) at
16:12:53.112 against a writer exit at 16:12:55.801. The engine now logs that
exit itself, so the qualitative half — was the game already gone? — no longer
needs any of this. The quantitative half still does: we learn the game had
exited, never when. Holding a handle on the identified process and reading
`GetProcessTimes` at writer exit would give the exact figure with no polling,
and is the same handle a process-exit stop signal would need.

That second session is the one where the wait became visible to the user, who
reported it unprompted as "very long". Corroborated by two independent sources
rather than our own log: Steam's `gameoverlay_ui.txt`, attached to pid 14552 —
the pid our refinement had named — logged the game gone at 14:32:50, and the
`EAAntiCheat` filter unloaded at 14:32:46. Our stop fired at 14:34:53.8, of
which 2 s is `stop_delay`.

**Reopened, then closed the same day: the writer stays the only signal.** The
two-minute session made the case for also ending on the identified game process
exiting, and Lot 3 had removed the old objection by naming that process
correctly, on 74 % of the rendering, two minutes before the writer let go.

Put to the user with the alternatives and the risks — a game that restarts its
own process mid-session (Forza did exactly this after a settings change) would
look like a quit, and a refinement that picked a satellite would end a session
early. **Decision: change nothing.** The variable delay is accepted in exchange
for an architecture with one signal in it, and the committed diagnostics are
enough to explain any particular wait after the fact.

So this is settled, not pending. Reopen it only on new evidence, and note that
the case for changing is weaker than a single two-minute session suggests: the
delay is random, so the same change that saves two minutes on one session saves
nothing on the next.
A few seconds of the wrong fan profile is not worth trading away the one signal
that has never been wrong.

### What everyone else does about it, researched 2026-09-10

The vendors with the most incentive to solve this have not solved it. They
recognise *known* games rather than detecting *a* game:

| Product | How it identifies a game |
| --- | --- |
| NVIDIA GeForce Experience | a curated database of supported titles, plus scanning configured folders |
| AMD Adrenalin | scans the usual game directories; anything elsewhere is added by hand |
| Discord | a hash table of executable names, with parts of the folder path to disambiguate generic ones |

All three are allow-lists, and all three fail on the same things: unusual
install paths, launchers, and executables with generic names. That is the
approach this project rejected at the start, and it is what the industry ships.

The exception is **Intel PresentMon**, which is open source and measures the
truth: it traces ETW frame-presentation events and knows which process is
actually rendering. It needs administrator rights or membership of the
*Performance Log Users* group, and an ETW trace session.

Two things follow. First, our position is already better than the allow-list
products: we read Microsoft's own Known Game List, which is the same kind of
database but maintained by the OS vendor and updated without us, plus a
presence signal none of them have. Second, there is nothing to copy.

### The one measurement available without privileges

Windows exposes per-process GPU counters — the ones Task Manager shows.
Verified on this machine, unelevated, `GPU Engine(*)` returned 807 valid
instances named `pid_<id>_..._engtype_3d`, correctly attributing load to
processes.

That is close to what PresentMon measures, without ETW or elevation. It cannot
find a game from nothing, but it can do something narrower and more useful:
**rank the candidates the Known Game List already produced**. A launcher stub
and an anti-cheat service sit near zero on the 3D engine; the game does not.

Caveat: this machine's account is a member of *Performance Log Users*, so the
reading proves the counters work here, not that they work for every account.
Any use of them needs a fallback.

### Why naming a game is hard, and why it stays optional

A game is not one process. It is an installer stub for dependencies, a splash
screen, a third-party launcher, an anti-cheat service, and somewhere among them
the executable a player would name. They share an install folder or a package
family, so they all match, and the satellites usually start first.

Three sessions, three different ways of getting it wrong: Starfield was named
after `gamelaunchhelper.exe`, Battlefield 6 after
`EAAntiCheat.GameServiceLauncher.exe` matched through its parent directory, and
Forza kept a process id that a mid-session restart had replaced.

Telling the real game apart would mean waiting to see which process actually
consumes CPU, GPU or memory. That is a heuristic, it is fragile, and it is the
kind of guessing this project set out to avoid.

**So naming stays deliberately best effort and is not a work item.** It feeds
the log and the placeholders; it never feeds detection. The only thing worth
fixing is the wording: the stop-edge line should not assert a process id it can
no longer vouch for.

### Windows tracks the title, not the process

Forza restarted itself mid-session after a settings change, replacing its
process. The writer never exited: Windows held the presence across a new
process id, and the fan profile stayed on Game throughout. Our own grace period
was never even reached. Worth knowing, because it means the writer is a
title-level signal rather than a process-level one.

Notes:

- **Why not just run the watcher elevated?** The configuration lives in
  `%APPDATA%` and names arbitrary programs to execute. An elevated watcher would
  turn that file into a way to run code as administrator with no prompt, for
  anything running as the user. The task bridge keeps the command somewhere a
  non-administrator cannot change it.
- Everything after this lot is polish on an unproven product until a real game
  session has driven real commands.

---

## Lot 2 — Configuration and how commands run · committed · `[x]` done

Goal: control over how several commands run for one event, and a configuration
that fails loudly and precisely when it is wrong.

**Staying on TOML.** The move to JSON was reconsidered and dropped. The
INI-like shape is the point, and TOML keeps three things JSON would have cost:
comments, no doubled separators in Windows paths thanks to single-quoted
literal strings, and — as it turns out — parse errors that are already better
than anything hand-rolled.

- [x] Several commands per event, at game start and at game stop
- [x] Validation errors that point at a line and column
- [x] Per-event execution mode: parallel or serial
- [x] A dedicated exit code when the configuration is invalid
- [x] Template polish: document every field in the shipped example
- [x] Stop the stop-edge log claiming a pid it can no longer vouch for

Done when: a config with several commands per event runs them in the configured
order or concurrently; an invalid config makes the program refuse to start, say
exactly where the problem is, and exit with the dedicated code.

**Verified 2026-09-10.** Two commands sleeping two seconds each: 4.54 s in
series, 2.22 s in parallel. Exit codes measured end to end: 3 for a missing
file, 4 for one that will not parse, 4 for one that fails validation, 0 for a
good one, 2 from the argument parser.

### What the parser already gives, verified 2026-09-09

A wrong value:

```
TOML parse error at line 6, column 14
  |
6 | stop_delay = 5s
  |              ^^
string values must be quoted, expected literal string
```

A misspelt key, thanks to `deny_unknown_fields`:

```
TOML parse error at line 2, column 1
  |
2 | log_levle = "info"
  | ^^^^^^^^^
unknown field `log_levle`, expected one of `stop_actions_on_exit`, `log_level`, `log_dir`, `log_keep_days`
```

Line, column, a caret under the offending token, and the list of valid names.
Nothing to build here.

What is still weak: semantic errors, checked after parsing, carry no position —
`detection.poll_interval must be greater than zero` says what but not where. It
could be improved with spanned deserialization, but a config this small does not
obviously need it. Left as polish.

### Exit codes

Everything currently exits with `1`, whatever went wrong. `clap` already returns
`2` for command-line misuse, so that value is spoken for.

| Code | Meaning |
| --- | --- |
| 0 | success |
| 2 | command line misuse (clap) |
| 3 | configuration file not found |
| 4 | configuration invalid: syntax or validation |
| 5 | another instance is already running |
| 1 | anything else |

### Parallel or serial

Serial already exists in substance through the per-action `wait` flag; this lot
turns it into an explicit per-event mode rather than a per-action detail.

Parallel needs a defined answer for failures: one command failing must not
prevent the others, and the log has to make clear which one failed. It also
needs a defined answer for the stop actions racing the next game start, since
`stop_delay` no longer serialises them.

## Lot 3 — Clean game naming · committed · `[x]` done

Goal: name the running game from the registry, while being explicit that failing
to name it is not a failure of the program.

- [x] Read Windows' Known Game List from `HKCU\System\GameConfigStore\Children`
- [x] Win32 titles matched by `MatchedExeFullPath` and `ExeParentDirectory`
- [x] Packaged Store and Game Pass titles matched by package family name
- [x] Generic directory names (`x64` and friends) ignored rather than trusted
- [x] `check <path>` to interrogate the list by hand
- [x] Say so plainly when no known game matched, in the log, at normal level
      rather than only in debug
- [x] Packaged titles matched wherever the Store installed them
- [x] Document that this is naming only, never detection
- [x] Tell the real game from its satellites, by GPU rendering load
- [x] Research what other products do, before building anything

Done when: a session with an unrecognised game runs the commands normally and
leaves a log line that unambiguously says no entry in Windows' known game list
matched.

**Reopened and closed 2026-09-10.** The research below found nothing worth
copying, so the narrower idea was built instead: rank the candidates the known
game list already produced by their GPU rendering load, once, a little way into
the session. It never promotes a process the list did not match, and no answer
is an expected outcome. `status` shows the ranking.

Notes:

- Most of this lot already exists; it was built alongside detection. What remains
  is the messaging, which is the part that matters for the stated goal.
- The unmatched case now logs a full sentence at info: the game was detected, no
  known game list entry matched any running process, and the actions still run
  with empty name placeholders.
- Two of two real games were named correctly, but only after packaged titles were
  supported: Starfield carries no executable path at all.

---

## Lot 4 — Logs that speak · committed · `[x]` done

Goal: one log that serves two readers. Someone who just wants to know what
happened reads it at `info` and sees plain sentences about games. A technician
reads the same log at `debug` and gets those same lines annotated, plus the
reasoning behind them.

- [x] Categories replace Rust module paths: `watcher`, `game`, `commands`
- [x] The level filter is generated from the category list, never hand-listed
- [x] A test refuses any logging call without a category
- [x] One `FormatEvent` for both sinks: aligned category column, colour only on a real terminal
- [x] Message carries the sentence, structured fields carry the technical annex
- [x] Fields printed only when the reader asked for `debug` or `trace`
- [x] All 30 call sites audited against the level contract below
- [x] The contract written in the README, where a reader of the log will find it

Done when: the same session reads correctly at `info` for a non-technical
reader and at `debug` for someone diagnosing, with no line written twice.

### Who each level is for

This is the contract. It is in the README too, because it is a promise made to
whoever opens the log, not an internal convention.

| Level | Reader | Rule |
| --- | --- | --- |
| `error` | anyone | Something needs you. Name the file or command and what to check. No jargon. |
| `warn` | technician | A degradation the program absorbed. May be technical. |
| `info` | anyone | The story of a session, in plain sentences. |
| `debug` | technician | *Why* the program did what it did, plus every `info` line annotated. |
| `trace` | technician | Raw measurements. |

**`info` is reserved for what this program is for**: a game was detected, named,
or lost, and the watcher started or stopped. Everything else had to earn its
place or move down. Three lines lost that argument — the writer path echoed at
startup, each command being started, each command's exit status — and are now
`debug`. That took `info` from 11 lines to 8, five of which are the detection
itself, which is the whole point: the lines that matter are no longer buried in
the ones that do not.

### Why the messages are not centralised

Asked whether to collect all log strings in one module, the way some codebases
do. **No**, and deliberately: in Rust that breaks locality — you jump to another
file to learn what a line says — and it turns literals the compiler checks into
runtime `format!` calls. It is not the idiom, and `tracing` is built against it.

What is centralised is the machinery: the categories, the filter built from
them, the single formatter, and the error types. That is where duplication
actually hurts.

The door is left open. If the tone of the public lines ever drifts, the 14
`info`/`error` messages can move behind an enum with a `Display` impl — one
file, reviewable in a pass, and unit-testable. Fourteen variants is defensible;
two hundred would not be. It can be added later without disturbing anything,
which is not true in the other direction. It is also the only way to localise
the log, which is worth having eventually but is nowhere near worth it now for
one user on an English-language codebase.

### The trap this lot walks past

An event whose target is not in `target::ALL` matches no filter directive and
is dropped **silently**. No warning, no error: the line simply never appears,
and nobody knows to look for it. Two things guard it — the filter is generated
from the same list the categories come from, so the two cannot drift, and
`every_log_site_declares_a_category` fails the build for any logging call
without a `target:`. That test earned its place immediately by catching its own
author's first version.

---

## Lot 5 — A Windows program with no window · committed · `[x]` done

Goal: the same program as today, minus the console. Nothing on screen — not a
window, not an icon, not a flash at logon. Behaviour identical, including what
happens when the user logs off during a game.

- [x] Build the watcher as a Windows-subsystem binary
- [x] Keep every CLI command working from a terminal, exit codes included — two binaries, see below
- [x] Invert the threading: message loop on the main thread, engine on a worker
- [x] A hidden top-level window whose procedure answers `WM_QUERYENDSESSION` and `WM_ENDSESSION`, so logoff and shutdown still run the stop actions
- [x] `install-task` points the logon task at the windowless binary, without `--hidden`
- [x] `install-task` stores an absolute configuration path — found by shipping a relative one
- [x] `--hidden` still accepted, ignored, so a task installed before this lot keeps starting; `hide_console` deleted
- [x] Verified: the task starts it with no window (`MainWindowHandle` 0); `status`, `validate` and the exit codes 0/2/3/4 are unchanged from a terminal; the session-end sequence drives a clean shutdown
- [ ] Still unverified: a real logoff **while a game is running**, which is the one case the window exists for

Done when: the logon task starts the watcher with nothing on screen, every CLI
command behaves exactly as before from a terminal, and a logoff during a game
restores the profile — which it does today.

### What the build looks like now

| Binary | Subsystem | For |
| --- | --- | --- |
| `gamemode-executor.exe` | `WINDOWS_CUI` | everything you type: `status`, `validate`, `check`, `trigger`, `init`, `install-task`, and `run` |
| `gamemode-executorw.exe` | `WINDOWS_GUI` | watching, and nothing else. What the logon task runs. |

Both are a few lines over the same library, and `src/service.rs` holds the one
implementation of "run the watcher" that they share. Verified by reading the
subsystem field out of each PE header rather than by trusting the build
settings.

### Verifying the session-end path without logging off

`WM_QUERYENDSESSION` and `WM_ENDSESSION` were sent to the running watcher's own
window, which is what Windows does at logoff. It answered 1, released
`WM_ENDSESSION` in 7 ms, logged `Stopped` and exited on its own.

One detour worth recording: `FindWindow` could not find the window, which looked
like a defect and was not. `FindWindow` resolves a class name through the global
atom table, and a class registered with `RegisterClassEx` is local to its
process. `EnumWindows` plus `GetClassName` asks each window directly and found
it immediately. The test method was wrong, not the code — worth knowing before
the same trap costs an hour during the icon lot.

### Why the window and the threading inversion are here and not with the icon

Because they preserve something the console version already does. `ctrlc`'s
Windows handler ignores the event type it is given (`os_handler(_: u32)`), so
today a logoff or shutdown reaches the watcher as a stop signal and, with
`stop_actions_on_exit`, the profile is restored. A Windows-subsystem process
with no window receives none of that. It is simply terminated, mid-game profile
and all.

The fix is a hidden top-level window whose procedure answers
`WM_QUERYENDSESSION` — which needs a thread pumping messages, which is the
threading inversion. It was scheduled with the icon because the icon needs it
too; it turns out "same behaviour as before" needs it first.

**Not a message-only window.** Those are documented as not receiving broadcast
messages, and `WM_QUERYENDSESSION` is one. So is `WM_SETTINGCHANGE`, which the
icon will need for the theme switch. A top-level window that is never shown
costs the same and receives both, and it is the window the icon will hang off
in the next lot.

### Keeping the CLI: three ways, one recommended

A Windows-subsystem process has no console. `status`, `validate`, `check` and
`trigger` all print to one, and scripts read `validate`'s exit code. The earlier
version of this plan said attaching to the parent console was the cheapest first
step. **It is not; it is the one that breaks a promise.**

**1. Two binaries, the `w` convention — recommended.** `gamemode-executor.exe`
stays the console program, every command, unchanged. `gamemode-executorw.exe`
is its Windows-subsystem twin: same crate, same code, `run` only, and the logon
task points at it. This is how Python ships (`python.exe` / `pythonw.exe`), and
Perl, and it exists precisely because the two subsystems cannot share one file.
Nothing to hack. The shell keeps waiting on the console binary, so exit codes,
pipes, redirection and colours all keep working. Cost: one more file to ship,
and both binaries are a few lines over the library that already exists.

**2. One binary, Windows subsystem, `AttachConsole(ATTACH_PARENT_PROCESS)`.**
Attach to the terminal it was launched from and reopen `stdout`. Single file.
But a shell does not wait for a GUI-subsystem process: the prompt comes back
before the output, which then prints over it — and `$LASTEXITCODE` and
`%ERRORLEVEL%` are not set, which breaks `validate` in any script, silently.
Working around that means `Start-Process -Wait` or `start /wait` on the user's
side. This is the hack the project said it would not do, and it trades Lot 2's
exit-code contract for one fewer file.

**3. Stay a console program, `FreeConsole()` at the top of `run --hidden`.**
Three lines. But the console is allocated before `main` runs, so a window
flashes at logon before it goes — and under Windows Terminal, whether the tab
closes at all when its only client detaches is untested. Cheapest, and it does
not meet "nothing on screen".

Decision pending. The rest of the lot is the same whichever way it goes.

### What stays out

No icon, no menu, nothing drawn. Also no `ShutdownBlockReasonCreate`
("Restoring the fan profile…" in Windows' shutdown screen) — worth having,
belongs with the rest of the shutdown work in Lot 9.

---

## Lot 6 — Notification area icon · committed · `[ ]`

Goal: a notification area icon with a small context menu: edit the
configuration, open the log, quit. Hung off the window Lot 5 created.

- [ ] Notification area icon, on the Lot 5 window
- [ ] Context menu: edit configuration · open log · quit
- [ ] Open both through the shell, so the user's own default program handles them
- [ ] Re-add the icon when Explorer restarts
- [ ] Follow the taskbar theme: `SystemUsesLightTheme`, re-read on `WM_SETTINGCHANGE` / `ImmersiveColorSet`
- [ ] Quit shuts the watcher down cleanly, stop actions included
- [ ] Icons from the Claude Design handoff: `.ico` files, eight sizes each, PNG frames, no C2PA payload — use those, not the standalone PNGs

Done when: the icon appears at logon in the right variant for the taskbar
theme, both menu entries open the right file in the user's chosen editor and
viewer, Quit exits cleanly, and the icon survives killing and restarting
Explorer.

Open on the artwork: the idle icon carries a diagonal slash, which in Windows
iconography reads as *disabled*. Idle is the state the program spends nearly all
its time in, and it means the opposite. Proposed: idle is the same gamepad
without the slash, in a duller grey; the slashed version is kept for a genuine
error state. The user's call.

On "the most modern APIs, no exotic libraries":

- For a notification area icon that means **`Shell_NotifyIcon` with
  `NOTIFYICON_VERSION_4`**. There is no newer replacement — WinUI and WinRT do
  not offer tray icons at all, so this is the current supported API rather than a
  legacy one. Modern here means the official Microsoft `windows` crate bindings,
  already a dependency, and no third-party tray wrapper.
- Opening the config and the log goes through **`ShellExecuteW`** with the default
  verb, which is exactly "let the user's own choice of program handle it".
- Impact was measured before committing: `user32`, `gdi32`, `shell32` and
  `combase` are **already loaded**, so no new DLL. Expect well under 1 MB of extra
  private bytes, no measurable CPU, and +30–60 KB of binary. Avoid
  `LoadIconMetric`, which would pull `comctl32.dll`.

Notes:

- The subsystem switch, the CLI decision and the threading inversion moved to
  Lot 5, where they are needed for reasons of their own. By the time this lot
  starts, the window and the message loop exist and have been proven with
  nothing on them. The engine stays pure and testable; the tray lives in its
  own module.
- Re-adding the icon on `TaskbarCreated` is not optional. Skipping it is the
  classic bug where the icon vanishes forever after Explorer restarts.

---

## Lot 7 — Tray experience · committed · `[ ]`

Goal: the icon says at a glance whether a game is detected, and which one.

- [ ] Two icons: idle, and game detected
- [ ] Switch icon on detection, both ways
- [ ] Non-clickable menu entry showing the active game
- [ ] Sensible text for that entry when the game could not be named

Done when: the icon changes within a poll of a game starting and stopping, and
the menu shows the game name, or says clearly that it is unknown.

Notes:

- Needs Lot 3 for the name and Lot 6 for the icon, so it comes last of the
  committed lots.
- Icons should carry 16/20/24/32 pixel sizes so they stay sharp on HiDPI.
- The disabled menu entry is the natural place for the "no known game matched"
  message from Lot 3, so the two lots should agree on one wording.

---

## Lot 8 — Distribution · proposed

**Partly done already, on 2026-09-14**: `scripts/build.ps1` runs the whole
checklist and produces the portable bundle in `dist/`, and `.vscode/tasks.json`
drives it. What is left for this lot is the part that needs a repository: a
workflow on a `v*` tag that runs that same script and attaches its output, and
version metadata compiled into the executables. The script was written first on
purpose — a release you cannot make by hand is not one CI can make for you.


Nothing ships anywhere yet: the repository is still local.

- Create the public GitHub repository and push
- Release workflow: on a `v*` tag, build and attach the executables
- Version and description metadata in the executable
- README install section pointing at a release rather than at `cargo build`

---

## Lot 9 — Robustness · proposed

- ~~Run the stop actions on logoff and shutdown~~ — moved into Lot 5, where it
  turned out to be a behaviour to preserve rather than one to add. What is left
  for here is the polish: `ShutdownBlockReasonCreate`, so Windows' shutdown
  screen says "Restoring the fan profile…" instead of naming the process.
- Reload the configuration without restarting, which pairs with the Lot 6 menu
  entry that opens it for editing.
- Behaviour across two games launched back to back.

---

## Lot 10 — Configuration window · proposed

The first lot with a real window. It edits the JSONC configuration through a UI,
so hand-editing mistakes — doubled backslashes above all — stop being possible.

Staying on TOML reopens something that was closed: `toml_edit` round-trips a file
while preserving comments and layout, so a configuration window could save
without destroying what the user wrote. JSON would have made that a rewrite from
scratch. Losing comments is still acceptable, but it may no longer be necessary.

To settle when it is taken: it supersedes the "edit configuration" menu entry
from Lot 6, which should then open the window rather than the shell.

---

## Lot 11 — Documentation for the people who use it · committed · `[x]` done

Goal: someone who has never seen this repository can get their own commands
running in five minutes, and someone curious can find out how it works without
reading Rust.

- [x] `docs/getting-started.md` — features and the shortest path to a result
- [x] `docs/recipes/` — one folder per worked example, each with a ready-made `config.toml`
- [x] `docs/how-it-works.md` — the mechanism, for a curious non-programmer
- [x] All three linked from the top of the README, above the reference material
- [x] The README keeps the reference and the reasoning; the new pages do not repeat them
- [x] FanControl lives in its recipe and nowhere else in the user docs; `tasks/` moved in beside it
- [x] Every shipped `config.toml` passes `validate` — they are files people copy, not illustrations

Done when: the two questions a newcomer actually asks — "how do I make it do my
thing" and "why are there two .exe files" — are answered without opening the
README.

### Numbered last, written now, and here is why

It sits at the end because that is where the user suggested it, and a third
renumbering would have cost more than it bought. It was **written now** anyway.

Documentation left until last is documentation written from memory, and memory
is where confident, wrong sentences come from. Everything in these pages was
measured or exercised in the sessions that produced lots 1 to 5, with the
evidence still in this plan. The remaining lots add to what is described — an
icon, a distribution — they do not contradict it.

### The audiences, kept apart on purpose

Three documents, three readers, no overlap:

| | Reader | Answers |
| --- | --- | --- |
| `docs/getting-started.md` | wants it working | what do I type, where do I put my commands, why is nothing happening |
| `docs/recipes/<job>/` | has a specific job in mind | give me the whole thing for my case, copy-paste |
| `docs/how-it-works.md` | curious, not a programmer, knows what a process is | how does it know a game is running, why two executables, why the wait after quitting |
| `README.md` | evaluating or contributing | the reference, the measurements, why every other approach was ruled out |

FanControl is confined to the recipe on purpose. It is the example that drove
the project, and left loose it would spread through every page until the program
looked like a FanControl accessory rather than something that runs commands.
`getting-started.md` names the *problem* — programs needing administrator rights
— and points at the recipe for the cure.

The middle one earns its place: the design has three things a user will
*notice* and misread as bugs — the unpredictable wait after quitting, the two
executables, and a game that is detected but not named. Each has a real reason.
Left unexplained, each looks like a defect.

### One folder per recipe

Each recipe is a folder holding its own `README.md` and a complete
`config.toml`, plus whatever else it needs — the FanControl one carries its two
Task Scheduler templates, which is why `tasks/` at the repository root is gone.
Adding a recipe adds a folder and one row in the index; nothing else moves, and
no existing recipe is touched.

The shipped `config.toml` files are checked with `validate` rather than written
and hoped for. They are files people copy over their own configuration, so a
typo in one is a typo in theirs.

### The maintenance rule

This lot is finished; the documentation is not, and never will be.

**Any lot that changes what the user sees updates these pages as part of being
done.** Not as a follow-up, not as a documentation pass at the end — in the same
commit. Lot 6 adds an icon and a menu: that is `getting-started.md`. Lot 8 ships
a release: that is both. Lot 9 changes shutdown behaviour: that is
`how-it-works.md`.

The alternative is documentation that describes a program that no longer
exists, which is worse than none, because nobody distrusts it until it has
already wasted their afternoon.

---

## Non-goals

Recorded so they stop coming back:

- No FanControl-specific integration. The program runs executables; that is all.
- No Windows service. Session 0 cannot see the desktop or the user's apps.
- No configuration GUI **before Lot 10**. Until then Lot 6 opens the file and the
  user brings their own editor.
- No allow-list of game executables, and no heuristics that guess at what a game
  is. Detection stays Windows' verdict.
- No telemetry, no network access.

---

## Assumptions still to verify

| Assumption | Status |
| --- | --- |
| FanControl switches profiles with `-c <profile>.json` | Settled. Unreachable directly because the binary requires elevation; bridged through a scheduled task, and the whole chain verified by reading the applied profile back from FanControl. |
| The presence writer is activated for games only | Notepad was a clean negative control; not proof for every application |
| Naming covers the titles actually played | Both named in the field: Farming Simulator 25 by executable path, Starfield by package family once the install-location assumption was removed. |
| The writer never blinks mid-session | Holds over two sessions including alt-tabs; `watch` polls at 100 ms, so a sub-100 ms dip could hide |

---

## Journal

**2026-09-14** — Everything this program registers now lives in a `\GameModeExecutor` folder in Task Scheduler instead of loose at the root: the watcher becomes `\GameModeExecutor\Watcher`, and the recipe's elevated tasks `\GameModeExecutor\FanControl <profile>`. `schtasks` creates the folder from the backslash in `/TN`, so nothing had to make it first; only the `<URI>` had to learn about it too. `uninstall-task` deliberately leaves the folder behind — a user's own elevated tasks live in it, and deleting a folder that still holds their work would be worse than leaving an empty one. Migrating the machine turned up something the move itself did not cause: all four FanControl tasks there carried `ExecutionTimeLimit PT1M`, three had `IgnoreNew`, and none set `AllowHardTerminate false` — the exact three settings the recipe calls out as failing silently. They worked only because FanControl already happened to be running, which makes the task's process short-lived; started cold, Task Scheduler would have killed FanControl after a minute and then ignored every later switch. So they were regenerated from the template rather than moved as they were. Also re-learned, the hard way, that `sed` eats backslashes: a substitution turned `GameModeExecutor\FanControl Game` into `GameModeExecutorFanControl Game` in three configuration files, which `validate` accepted happily because it is a perfectly good string.

**2026-09-14** — Lot 5 built: the watcher is now a windowless program, and the console one keeps every command. Two binaries on the `pythonw` convention, sharing one library through a new `service` module, verified by reading the subsystem out of each PE header rather than trusting the build. The session-end path was tested without logging off, by sending `WM_QUERYENDSESSION` and `WM_ENDSESSION` to the watcher's own window: it answered 1, released in 7 ms, logged `Stopped` and exited by itself. Two things worth keeping. `FindWindow` could not find that window and looked like a bug — it resolves class names through the global atom table, and `RegisterClassEx` registers locally, so `EnumWindows` is the way to find it; the test was wrong, not the code. And `install-task` shipped a *relative* configuration path into the task, which would have exited 3 at the next logon with nobody watching; found by reading back what was actually registered instead of trusting the success message. Now absolute, with a test. Still unverified: a real logoff while a game is running, which is the single case the window exists for.

**2026-09-14** — The tray icon lot split in two at the user's request: the Windows-subsystem work is Lot 5, the icon itself Lot 6, everything after shifts up one. The split exposed something the old lot had wrong. Checking what "same behaviour as before" actually covers turned up `ctrlc`'s Windows handler, which signals on every control event including logoff and shutdown — so the console watcher restores the fan profile when the user logs off mid-game, and a windowless process would not. That pulls a hidden top-level window and the threading inversion into Lot 5 for a reason that has nothing to do with the icon, and rules out a message-only window, which does not receive the broadcasts. The old plan also called `AttachConsole` the cheapest way to keep the CLI; reversed, because a shell does not wait for a GUI-subsystem process and `validate`'s exit code would silently stop reaching scripts. Three options recorded, two binaries recommended, decision pending.

**2026-09-10** — Lot 4, logging, written and moved ahead of the tray icon so the icon is written in the finished vocabulary rather than converted after. The categories were the visible complaint — `game_mode_executor::engine` means nothing to a reader — but the useful part was the rule that came with it: `info` belongs to what the program is for, and everything else has to earn its place. Three lines did not, and the detection lines stopped being buried. The mechanism is `tracing` used as intended rather than as `println!`: the message is the sentence, the fields are the technical annex, and the level decides whether the annex prints. One event, two readings, nothing written twice — which also retired the duplicate `info`/`debug` pair I had proposed a few hours earlier. Declined to centralise the message strings, which is an anti-pattern in Rust; centralised the machinery instead, and recorded what would justify revisiting that. Renumbered lots 4 to 8 into 5 to 9.

**2026-09-10** — The logon task leaves a visible, permanently blank console window, and both halves of that were ours. `--hidden` was overloaded: it hid the window *and* switched off console logging, so nothing was ever written to the window it failed to hide. And it fails to hide under Windows Terminal, the Windows 11 default: the process gets a ConPTY, `GetConsoleWindow` returns the pseudo-console's already-invisible window rather than the Terminal one, so the call succeeds, hides nothing, and reports nothing. Console logging is now unconditional — writing to a console nobody can see costs nothing, and when hiding fails the window is at least useful. The flag keeps only its "try to hide" meaning, still correct under conhost. Lot 6 is the real fix: a Windows-subsystem program owns no console to hide.

**2026-09-10** — Lots 2 and 3 tested in real conditions, on Battlefield 6. Lot 3 did exactly what it exists for: the session opened named after `EAAntiCheat.GameServiceLauncher.exe`, and 21.4 s later — the configured 20 s plus the 1 s sample plus overhead — it corrected itself to `bf6.exe` on 74 % of the rendering, a name that then survived into the stop message. That is the first time the GPU ranking has run against a real title; before this it was only unit tests and a `status` display. Lot 2 held up too, both modes visible in the timestamps rather than merely labelled, and exit code 5 turned up unplanned in Task Scheduler when a restart raced the old instance. The session also produced the thing worth keeping: the user reported the stop as very long, and it was — 2 min 4 s, of which 2 s were ours. Establishing that needed Steam's logs because our own said nothing about when the writer exited, so the engine now records that moment and whether the named game was already gone. The measurement retired the per-title theory of the post-quit delay and reopened whether the identified process should also end a session; see the section above rather than deciding from one session.

**2026-09-10** — Lot 3 reopened, researched, and closed. Nothing in the industry was worth copying: the three products with the most incentive all ship allow-lists. What came out of the research instead was a measurement available without privileges — the per-process GPU counters — and a narrower way to use it. Rather than trying to find a game, it ranks the candidates the known game list already matched, once, twenty seconds into a session. The wait happens on the writer's handle rather than in a sleep, so it stays blind to nothing. Lots 1, 2 and 3 are done and tagged.

**2026-09-10** — Measured the delay after quitting a game, and it is not where I guessed. Starfield's process was gone 2.8 s after the click; Windows then held the presence writer for another 52.2 s. I had argued the game was probably slow to exit and dismissed the cloud-save-sync explanation; the measurement says the opposite, and that explanation is now the one still standing. It predicts a much smaller gap on a Steam title, which is worth checking. The same run exposed a naming defect: identify returns the first matching process, and gamelaunchhelper.exe shares Starfield's package family, so the session was named after a stub that died 0.4 s later.

**2026-09-10** — The installed logon task carried three schtasks defaults that were wrong for a watcher meant to run forever, the worst being ExecutionTimeLimit PT72H, which would have had Windows kill it after three days. The two battery settings would have stopped it on an unplugged laptop. Ironic, since the FanControl task definitions written by hand had all three right. install-task now registers from an XML definition like those, and gained a restart-on-failure. Logs moved to local timestamps via GetLocalTime, and the daily rotation was dropped: it only ever bought filenames dated in UTC, which was the confusion being fixed. log_keep_days goes with it, so an existing configuration has to be replaced rather than kept.

**2026-09-10** — Reboot confirmed the autostart chain: FanControl 28 s after boot, the watcher 38 s, both from their own logon tasks and in the right order, with only the hardware-touching tasks elevated. Noticed while reading the log that timestamps and the daily file rotation are UTC, so an event at 00:46 local is filed under the previous day at 22:46. Harmless mechanically, but it works against a log meant to be read by one person correlating it with what they just did.

**2026-09-10** — The naming fix holds in the field. Starfield is now named on both edges through its package family, with the image path reported as C:\Games\Starfield\Content\Starfield.exe. Worth recording that PowerShell disagrees: Process.Path reports the WindowsApps path for the same process, because .NET goes through GetModuleFileNameEx while QueryFullProcessImageNameW with PROCESS_NAME_WIN32 resolves the junction. The PowerShell reading had briefly seemed to contradict the diagnosis. Noted an unmeasured gap: closing a game felt slow to register, and the time between the user quitting and Windows releasing the presence writer has never been measured.

**2026-09-10** — First real game sessions. The profile switching worked for both, and Farming Simulator 25 was named correctly through the executable path. Starfield was not named, which exposed a real bug: the packaged branch only asked a process for its package family name when its image path sat under WindowsApps. The Store lets a game be installed anywhere — here C:\Games\Starfield — and the WindowsApps entry is then a junction that Windows resolves, so the running process reports the real path and never looked packaged. The filter was an optimisation that quietly encoded an assumption about install locations. Removed: both questions are now asked on a single process handle, which is also cheaper than the two opens it replaced. Added check --pid, which shows what the naming code actually reads, since a process that cannot be opened was previously indistinguishable from one Windows does not list.

**2026-09-09** — The scheduled task bridge works. With both tasks registered, a simulated game session drove FanControl from Quiet to Game and back, confirmed by reading CurrentConfigFileName out of FanControl's own CACHE file rather than trusting the exit code. About 2.4 s from game start to the profile being applied, 6.6 s back, 5 s of which is the configured grace period. The technical core of Lot 1 is closed; what is left is a real game and a reboot.

**2026-09-09** — Reversed the move to JSON: staying on TOML, for the INI-like shape. That keeps comments, keeps Windows paths readable in single-quoted literal strings, and removes the comment-blanking trick that JSONC would have needed. Checked what the parser already reports and found the line-and-column requirement already met, with a caret under the offending token and the list of valid field names for a misspelt key, so that item was marked done rather than built. Lot 2 shrinks to the execution mode, the exit codes and template polish. Side effect on Lot 10: toml_edit can round-trip a file without destroying comments, so a configuration window may not have to lose them after all.

**2026-09-09** — Discovered that files written to %APPDATA% during these sessions land in an MSIX package container and are invisible to a normal shell, so the working config and the task definitions had to move into the repository, which is not redirected. The program behaviour verified so far still stands — the presence writer, the scheduled tasks and the elevation failure are all system-wide facts — but anything checked through a file under %APPDATA% was checked inside that container.

**2026-09-09** — Lot 1 hit the finding it existed to find. FanControl cannot be started by the watcher at all: its manifest requires administrator, so CreateProcess fails with error 740 regardless of the command line. The README had been promising exactly that command since the first sketch. Fixed by going through a scheduled task registered with highest privileges, which the unelevated watcher triggers with schtasks /Run and which raises no UAC prompt. Running the watcher elevated was rejected: it would turn a user-writable config file into a local privilege escalation. Also landed the two remaining code items — file logging on by default, and log lines that say GAME DETECTED and GAME NO LONGER DETECTED, including an explicit sentence when Windows' known game list matched nothing.

**2026-09-09** — Comment preservation dropped as a requirement. The configuration is a convenience, so losing the user's own comments on a rewrite is accepted. That removes the only real tension between Lot 2 and Lot 10 and means no comment-preserving serializer is needed. A rewrite should still re-emit the documented header, so the file keeps explaining its own doubled backslashes instead of decaying into bare JSON.

**2026-09-09** — Lot 2 decided: JSONC, so the template can explain its own
fields and warn about doubled backslashes. Added a validation step and a
dedicated exit code for an invalid configuration. Found that comments need no
dependency at all — blanking them with spaces of the same length keeps
`serde_json`'s error line and column pointing at the real position in the user's
file. Added Lot 10, a configuration window, which is where hand-editing mistakes
stop being possible; noted that it must not silently destroy the comments Lot 2
introduces.

**2026-09-09** — Plan restructured around five lots defined by their outcome
rather than by the order things happened to get built: correct console
behaviour, JSON configuration with parallel or serial commands, clean game
naming, the move to a windowed program with a tray icon, then the tray
experience. Distribution and robustness added as proposals. Recorded that Lot 3
is largely already implemented and that its remaining value is the messaging
when nothing matches.

**2026-09-09** — Lot 1 work. FanControl turned out to be installed after all,
portable under OneDrive, which is why the usual locations came up empty. Its CLI
matched the assumption, so the README example stands. Working config written and
the pipeline proven with placeholder actions. Fixed the grace period, which
rounded up to a whole number of polls instead of honouring `stop_delay`: it now
waits the shorter of a poll and the time remaining, measured at 5.01 s for a 5 s
setting.

**2026-09-09** — Detection settled. Ruled out the Game Mode APIs (deprecated
since 1809, callable only from inside the game), Xbox Mode (a shell mode with
private APIs), and `GameList` (restricted capability). A custom `IPresenceWriter`
turned out to be impossible: the registration key is owned by TrustedInstaller
and neither Administrators nor SYSTEM can write it. Observing the shipped writer
instead works and costs nothing. Measured its activation (40 ms) and exit (under
20 ms after release), then validated it over two real game sessions. Added
packaged-title naming after Starfield turned out to carry no executable path at
all. Removed the process allow-list and the interim full-screen detector.
