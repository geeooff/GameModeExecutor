# Development plan

Working document. Updated as work lands, not written once and forgotten.

Status: `[ ]` todo · `[~]` in progress · `[x]` done · `[-]` dropped, with a reason.

Lots 1 to 5 are **committed**. Lots 6 and 7 are **proposed** and carry only
enough detail to decide whether they are wanted: writing detail is how scope
grows.

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

**Dependency order:** 1 → 2 → 4, with 3 independent, and 5 needing both 3 and 4.

---

## Lot 1 — Correct console behaviour · committed · `[~]`

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

So it is neither a Game Pass trait nor a Windows constant: Forza is a Game Pass
title and releases in six seconds. **Starfield is the outlier**, for reasons we
have not identified and are not worth chasing. Both explanations offered
earlier are dead: the game closing slowly (its process was gone in 2.8 s) and
cloud save synchronisation as a store-wide behaviour.

**Decided: the writer stays the only trigger.** Acting on the game process
exiting would buy a few seconds on typical titles, at the cost of making
detection depend on naming — and naming is exactly the part that keeps failing.
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

## Lot 3 — Clean game naming · committed · `[~]`

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
- [-] Chase the real game among a title's satellite processes — dropped: it
      would need a resource-consumption heuristic, which is exactly the kind of
      guessing this project avoids

Done when: a session with an unrecognised game runs the commands normally and
leaves a log line that unambiguously says no entry in Windows' known game list
matched.

Notes:

- Most of this lot already exists; it was built alongside detection. What remains
  is the messaging, which is the part that matters for the stated goal.
- The unmatched case now logs a full sentence at info: the game was detected, no
  known game list entry matched any running process, and the actions still run
  with empty name placeholders.
- Two of two real games were named correctly, but only after packaged titles were
  supported: Starfield carries no executable path at all.

---

## Lot 4 — Windows program with a tray icon · committed · `[ ]`

Goal: stop being a console program. No window, just a notification area icon with
a small context menu: edit the configuration, open the log, quit.

- [ ] Switch to the Windows subsystem
- [ ] Invert the threading: message loop on the main thread, engine on a worker
- [ ] Notification area icon
- [ ] Context menu: edit configuration · open log · quit
- [ ] Open both through the shell, so the user's own default program handles them
- [ ] Re-add the icon when Explorer restarts
- [ ] Quit shuts the watcher down cleanly, stop actions included

Done when: the program runs with no console, the icon appears at logon, both menu
entries open the right file in the user's chosen editor and viewer, Quit exits
cleanly, and the icon survives killing and restarting Explorer.

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

Decision to take: switching to the Windows subsystem silences `status`, `check`
and `validate`, which print to a console that no longer exists. Either attach to
the parent console when started from one, or keep a separate console binary for
those commands. Cheapest first step is the former.

Notes:

- The threading inversion is the only structurally invasive step: the tray window
  needs a thread that pumps messages, and today the main thread blocks in
  `WaitForMultipleObjects`. The engine stays pure and testable; the tray lives in
  its own module.
- Re-adding the icon on `TaskbarCreated` is not optional. Skipping it is the
  classic bug where the icon vanishes forever after Explorer restarts.

---

## Lot 5 — Tray experience · committed · `[ ]`

Goal: the icon says at a glance whether a game is detected, and which one.

- [ ] Two icons: idle, and game detected
- [ ] Switch icon on detection, both ways
- [ ] Non-clickable menu entry showing the active game
- [ ] Sensible text for that entry when the game could not be named

Done when: the icon changes within a poll of a game starting and stopping, and
the menu shows the game name, or says clearly that it is unknown.

Notes:

- Needs Lot 3 for the name and Lot 4 for the icon, so it comes last of the five.
- Icons should carry 16/20/24/32 pixel sizes so they stay sharp on HiDPI.
- The disabled menu entry is the natural place for the "no known game matched"
  message from Lot 3, so the two lots should agree on one wording.

---

## Lot 6 — Distribution · proposed

Nothing ships anywhere yet: the repository is still local.

- Create the public GitHub repository and push
- Release workflow: on a `v*` tag, build and attach the executables
- Version and description metadata in the executable
- README install section pointing at a release rather than at `cargo build`

---

## Lot 7 — Robustness · proposed

- Run the stop actions on logoff and shutdown. Becomes cheap once Lot 4 gives us
  a window: `WM_QUERYENDSESSION` / `WM_ENDSESSION`.
- Reload the configuration without restarting, which pairs with the Lot 4 menu
  entry that opens it for editing.
- Behaviour across two games launched back to back.

---

## Lot 8 — Configuration window · proposed

The first lot with a real window. It edits the JSONC configuration through a UI,
so hand-editing mistakes — doubled backslashes above all — stop being possible.

Staying on TOML reopens something that was closed: `toml_edit` round-trips a file
while preserving comments and layout, so a configuration window could save
without destroying what the user wrote. JSON would have made that a rewrite from
scratch. Losing comments is still acceptable, but it may no longer be necessary.

To settle when it is taken: it supersedes the "edit configuration" menu entry
from Lot 4, which should then open the window rather than the shell.

---

## Non-goals

Recorded so they stop coming back:

- No FanControl-specific integration. The program runs executables; that is all.
- No Windows service. Session 0 cannot see the desktop or the user's apps.
- No configuration GUI **before Lot 8**. Until then Lot 4 opens the file and the
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

**2026-09-10** — Measured the delay after quitting a game, and it is not where I guessed. Starfield's process was gone 2.8 s after the click; Windows then held the presence writer for another 52.2 s. I had argued the game was probably slow to exit and dismissed the cloud-save-sync explanation; the measurement says the opposite, and that explanation is now the one still standing. It predicts a much smaller gap on a Steam title, which is worth checking. The same run exposed a naming defect: identify returns the first matching process, and gamelaunchhelper.exe shares Starfield's package family, so the session was named after a stub that died 0.4 s later.

**2026-09-10** — The installed logon task carried three schtasks defaults that were wrong for a watcher meant to run forever, the worst being ExecutionTimeLimit PT72H, which would have had Windows kill it after three days. The two battery settings would have stopped it on an unplugged laptop. Ironic, since the FanControl task definitions written by hand had all three right. install-task now registers from an XML definition like those, and gained a restart-on-failure. Logs moved to local timestamps via GetLocalTime, and the daily rotation was dropped: it only ever bought filenames dated in UTC, which was the confusion being fixed. log_keep_days goes with it, so an existing configuration has to be replaced rather than kept.

**2026-09-10** — Reboot confirmed the autostart chain: FanControl 28 s after boot, the watcher 38 s, both from their own logon tasks and in the right order, with only the hardware-touching tasks elevated. Noticed while reading the log that timestamps and the daily file rotation are UTC, so an event at 00:46 local is filed under the previous day at 22:46. Harmless mechanically, but it works against a log meant to be read by one person correlating it with what they just did.

**2026-09-10** — The naming fix holds in the field. Starfield is now named on both edges through its package family, with the image path reported as C:\Games\Starfield\Content\Starfield.exe. Worth recording that PowerShell disagrees: Process.Path reports the WindowsApps path for the same process, because .NET goes through GetModuleFileNameEx while QueryFullProcessImageNameW with PROCESS_NAME_WIN32 resolves the junction. The PowerShell reading had briefly seemed to contradict the diagnosis. Noted an unmeasured gap: closing a game felt slow to register, and the time between the user quitting and Windows releasing the presence writer has never been measured.

**2026-09-10** — First real game sessions. The profile switching worked for both, and Farming Simulator 25 was named correctly through the executable path. Starfield was not named, which exposed a real bug: the packaged branch only asked a process for its package family name when its image path sat under WindowsApps. The Store lets a game be installed anywhere — here C:\Games\Starfield — and the WindowsApps entry is then a junction that Windows resolves, so the running process reports the real path and never looked packaged. The filter was an optimisation that quietly encoded an assumption about install locations. Removed: both questions are now asked on a single process handle, which is also cheaper than the two opens it replaced. Added check --pid, which shows what the naming code actually reads, since a process that cannot be opened was previously indistinguishable from one Windows does not list.

**2026-09-09** — The scheduled task bridge works. With both tasks registered, a simulated game session drove FanControl from Quiet to Game and back, confirmed by reading CurrentConfigFileName out of FanControl's own CACHE file rather than trusting the exit code. About 2.4 s from game start to the profile being applied, 6.6 s back, 5 s of which is the configured grace period. The technical core of Lot 1 is closed; what is left is a real game and a reboot.

**2026-09-09** — Reversed the move to JSON: staying on TOML, for the INI-like shape. That keeps comments, keeps Windows paths readable in single-quoted literal strings, and removes the comment-blanking trick that JSONC would have needed. Checked what the parser already reports and found the line-and-column requirement already met, with a caret under the offending token and the list of valid field names for a misspelt key, so that item was marked done rather than built. Lot 2 shrinks to the execution mode, the exit codes and template polish. Side effect on Lot 8: toml_edit can round-trip a file without destroying comments, so a configuration window may not have to lose them after all.

**2026-09-09** — Discovered that files written to %APPDATA% during these sessions land in an MSIX package container and are invisible to a normal shell, so the working config and the task definitions had to move into the repository, which is not redirected. The program behaviour verified so far still stands — the presence writer, the scheduled tasks and the elevation failure are all system-wide facts — but anything checked through a file under %APPDATA% was checked inside that container.

**2026-09-09** — Lot 1 hit the finding it existed to find. FanControl cannot be started by the watcher at all: its manifest requires administrator, so CreateProcess fails with error 740 regardless of the command line. The README had been promising exactly that command since the first sketch. Fixed by going through a scheduled task registered with highest privileges, which the unelevated watcher triggers with schtasks /Run and which raises no UAC prompt. Running the watcher elevated was rejected: it would turn a user-writable config file into a local privilege escalation. Also landed the two remaining code items — file logging on by default, and log lines that say GAME DETECTED and GAME NO LONGER DETECTED, including an explicit sentence when Windows' known game list matched nothing.

**2026-09-09** — Comment preservation dropped as a requirement. The configuration is a convenience, so losing the user's own comments on a rewrite is accepted. That removes the only real tension between Lot 2 and Lot 8 and means no comment-preserving serializer is needed. A rewrite should still re-emit the documented header, so the file keeps explaining its own doubled backslashes instead of decaying into bare JSON.

**2026-09-09** — Lot 2 decided: JSONC, so the template can explain its own
fields and warn about doubled backslashes. Added a validation step and a
dedicated exit code for an invalid configuration. Found that comments need no
dependency at all — blanking them with spaces of the same length keeps
`serde_json`'s error line and column pointing at the real position in the user's
file. Added Lot 8, a configuration window, which is where hand-editing mistakes
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
