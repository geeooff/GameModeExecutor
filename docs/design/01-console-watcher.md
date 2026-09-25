# Lot 1 — A console watcher that is correct and boring

**Goal.** The technical core as a console program: configured commands run
when they should and nothing else happens, with a log that says when a game
was detected and when it was no longer.

**Done when:** a real game session drives the configured commands, start and
stop, started automatically at logon, with a log that shows what happened and
no known defects. Closed 2026-09-10.

- [x] Detection by presence writer lifetime: idle poll, then park on the handle
- [x] Action runner: args, working dir, env, no-window, wait, timeout
- [x] `stop_delay` honoured to the configured value, not rounded up to whole polls
- [x] Configuration found without `--config`; `validate` and `init`
- [x] Per-user logon task via `schtasks`, no elevation, registered from XML
- [x] Single instance per session
- [x] File logging on by default, timestamps in local time
- [x] Real commands wired through a scheduled task, the whole chain verified
- [x] Real game sessions: detected, named, commands run on both edges
- [x] Logon task confirmed across a reboot
- [x] Measured what happens between closing a game and the writer being released
- [-] Act on the game process exiting as well as the writer — dropped, see below

## Reaching a program that requires administrator rights

FanControl, the program that drove the project, declares
`requestedExecutionLevel level="requireAdministrator"` in its manifest because
it talks to hardware. `CreateProcess` from an unelevated parent therefore fails
with error 740, `ERROR_ELEVATION_REQUIRED`, whatever the command line. Found
by running it, not by reading about it.

The bridge is one scheduled task per role, registered once with *run with
highest privileges*, triggered by the watcher with `schtasks /Run`. Triggering
a task needs no elevation and raises no UAC prompt.

**Why not run the watcher elevated instead?** The configuration lives in the
user's profile and names arbitrary programs to execute. An elevated watcher
would turn that file into a way to run code as administrator with no prompt,
for anything running as the user. The task bridge keeps the command somewhere
a non-administrator cannot change it.

**Chain verified end to end, 2026-09-09**, with the change read back from
FanControl's own `CACHE` file rather than assumed:

| Moment | Event |
| --- | --- |
| 23:49:52.754 | presence writer appears |
| 23:49:53.680 | game detected, task triggered, `schtasks` returns 0 |
| 23:49:55.123 | FanControl is on the gaming configuration |
| 23:49:57.762 | presence writer exits |
| 23:50:02.772 | game no longer detected after the 5 s grace, task triggered |
| 23:50:04.349 | FanControl is back on the everyday configuration |

About 2.4 s from game start to the configuration being applied, and 6.6 s from
game end to it being restored, of which 5 s was the configured `stop_delay`.
FanControl kept the same PID throughout: nothing started a second instance.

## Where the delay after quitting a game goes

Measured 2026-09-10 on Starfield, with `presence-probe watch` tracking both the
writer and the game process, and the moment of quitting noted by hand:

| Segment | Duration |
| --- | --- |
| click quit to the game process being gone | 2.8 s |
| **game process gone to Windows releasing the writer** | **52.2 s** |
| writer released to the stop actions | 1.9 s (`stop_delay`) |
| the action to the configuration applied | about 1.4 s |

Roughly a minute from quitting to the fans changing, and **all but six seconds
of it is Windows**. Tuning `stop_delay` is noise at this scale.

Two more titles, measured the same way the same day, settle where that 52 s
comes from:

| Title | Store | Game process gone to writer released |
| --- | --- | --- |
| Starfield | Game Pass | **52.2 s** |
| Forza Horizon 6 | Game Pass | 6.2 s |
| Battlefield 6 | Steam | 4.4 s |
| Battlefield 6, second session | Steam | **about 2 min 2 s** |
| Starfield, second session | Game Pass | **2.7 s** |

The explanations first offered are both dead: the game closing slowly (its
process was gone in 2.8 s), and cloud save synchronisation as a store-wide
behaviour.

**The last two rows kill the per-title theory outright.** The first three read
as "Starfield is the outlier", one number per title. Measured a second time,
both titles moved — in opposite directions. Same titles, same stores, same
machine, same afternoon. The delay is not a property of the title: no table of
titles will ever predict it, there is nothing to tune, and a session that stops
promptly proves nothing about the next one. Windows releases the writer when it
decides to.

**Corrected 2026-09-25: it decides on the user's next input.** Four sessions
of GTA Online, the game's exit read from the Rockstar launcher's own log
(*Game exited with code 0x0*, BattlEye's driver unloading the same second),
the release from the watcher's:

| Date | After quitting | Game gone to writer released |
| --- | --- | --- |
| 2026-09-23 | nobody at the machine; the screen off by its 5-minute timeout | 2 h 22 min, 23 s after the screen came back on input |
| 2026-09-24 | hands off four minutes, screen on, then the mouse | 4 min 14 s, seconds after the mouse |
| 2026-09-24 | the machine used throughout | 16.8 s |
| 2026-09-25 | as the second, an Xbox party chat open and pinned | 4 min 5 s, seconds after the mouse |

Windows releases the writer some twenty seconds after the first keyboard or
mouse input that follows the game's exit, and not before — the screen and
the party chat change nothing. The per-title theory stays dead; what the two
sessions of each title above differed in is most likely when the maintainer
next touched the machine, which was not noted then — inferred, not
measured. [Lot 18](18-game-gone-nobody-there.md) has the details.

The second sessions were measured from independent sources, since the log did
not yet record the writer's exit: Steam's `gameoverlay_ui.txt` for Battlefield
6, and the AppX container destruction event (`Microsoft-Windows-AppModel-Runtime/Admin`,
event 217) for Starfield. The engine now logs that exit itself, so the
qualitative half — was the game already gone? — needs none of this. The
quantitative half still would: the log learns the game had exited, never when.
Holding a handle on the identified process and reading `GetProcessTimes` at
writer exit would give the exact figure with no polling.

**Reopened, then closed the same day: the writer stays the only signal.** The
two-minute session made the case for also ending the session when the
identified game process exits, and Lot 3 had removed the old objection by
naming that process correctly. The risks: a game that restarts its own process
mid-session (Forza did exactly this after a settings change) would look like a
quit, and a refinement that picked a satellite would end a session early.
**Decision: change nothing.** A variable delay is accepted in exchange for an
architecture with one signal in it, and the committed diagnostics explain any
particular wait after the fact. The delay is random, so a change that saves
two minutes on one session saves nothing on the next; a few seconds of the
wrong fan profile is not worth trading away the one signal that has never been
wrong. Reopen only on new evidence.

**New evidence, 2026-09-25.** The delay is not "random, seconds to two
minutes": it lasts as long as nobody touches the machine, 2 h 22 min
measured, and the decision above was weighed against a few seconds of the
wrong fan profile. It is reopened as a proposal, [Lot 18](18-game-gone-nobody-there.md)
— not decided.

## What everyone else does about it

Researched 2026-09-10. The vendors with the most incentive to solve this have
not solved it. They recognise *known* games rather than detecting *a* game:

| Product | How it identifies a game |
| --- | --- |
| NVIDIA GeForce Experience | a curated database of supported titles, plus scanning configured folders |
| AMD Adrenalin | scans the usual game directories; anything elsewhere is added by hand |
| Discord | a hash table of executable names, with parts of the folder path to disambiguate generic ones |

All three are allow-lists, and all three fail on the same things: unusual
install paths, launchers, executables with generic names. That is the approach
this project rejected at the start, and it is what the industry ships.

The exception is **Intel PresentMon**, open source, which measures the truth:
it traces ETW frame-presentation events and knows which process is actually
rendering. It needs administrator rights or membership of *Performance Log
Users*, and an ETW trace session.

Two things follow. This project's position is already better than the
allow-list products: it reads Microsoft's own Known Game List, the same kind
of database but maintained by the OS vendor, plus a presence signal none of
them have. And there is nothing to copy.

## The one measurement available without privileges

Windows exposes per-process GPU counters — the ones Task Manager shows.
Verified unelevated: `GPU Engine(*)` returned 807 valid instances named
`pid_<id>_..._engtype_3d`, correctly attributing load to processes.

That is close to what PresentMon measures, without ETW or elevation. It cannot
find a game from nothing, but it can do something narrower and more useful:
**rank the candidates the Known Game List already produced**. A launcher stub
and an anti-cheat service sit near zero on the 3D engine; the game does not.
Lot 3 uses exactly this.

Caveat: the account this was verified on is a member of *Performance Log
Users*, so the reading proves the counters work there, not for every account.
Any use of them needs a fallback, and has one.

## Windows tracks the title, not the process

Forza restarted itself mid-session after a settings change, replacing its
process. The writer never exited: Windows held the presence across a new
process id, and the fan profile stayed on Game throughout; the grace period
was never even reached. The writer is a title-level signal, not a
process-level one.
