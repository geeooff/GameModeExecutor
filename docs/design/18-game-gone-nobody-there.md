# Lot 18 — When the game is gone and nobody is there

**Status: proposed 2026-09-25, not decided.** Written at the maintainer's
request as a proposal: a finding, the question it raises, and the options
it opens. Nothing here is planned, and the first thing to settle is whether
the program should do anything at all. Until then there is no *done when*.

## What was found

On 2026-09-23 the maintainer quit GTA Online, left the machine, and found
the "game stopped" commands had waited until they came back, more than two
hours later. The watcher had done what it is built to do: end the session
when Windows releases its presence writer. Three more sessions were then
played to find what Windows waits for, the game's exit read each time from
the Rockstar launcher's own log (*Game exited with code 0x0*, BattlEye's
driver unloading the same second), the release from the watcher's log at
`debug`, the screen from `Microsoft-Windows-Kernel-Power` event 566, the
input from the maintainer's own notes:

| Date | After quitting | Game gone | Writer released | In between |
| --- | --- | --- | --- | --- |
| 2026-09-23 | nobody at the machine; screen off by its 5-minute timeout at 21:11:09, back on input at 23:28:21 | 21:06:04 | 23:28:44 | **2 h 22 min** |
| 2026-09-24, A | hands off four minutes, screen on, then the mouse at about 02:37 | 02:33:08 | 02:37:22 | **4 min 14 s** |
| 2026-09-24, B | the machine used throughout | 16:53:04 | 16:53:20 | **16.8 s** |
| 2026-09-25, C | as A, an Xbox party chat open and pinned in front; the mouse at about 02:13 | 02:09:18 | 02:13:23 | **4 min 5 s** |
| 2026-09-25, Battlefield 6 | the machine used throughout: the log opened from the menu, the game relaunched | 15:05:04 | 15:05:22 | **18 s** |
| 2026-09-25, American Truck Simulator | the machine used throughout, the next game launched from Steam | 16:03:54 | 16:03:55 | **0.6 s** |
| 2026-09-25, Euro Truck Simulator 2 | the same | 16:05:04 | 16:06:03 | **58.5 s** |
| 2026-09-25, American Truck Simulator | the same | 16:07:34 | 16:08:25 | **50.3 s** |
| 2026-09-25, Euro Truck Simulator 2 | the same | 16:10:56 | 16:10:57 | **1.0 s** |
| 2026-09-25, Euro Truck Simulator 2, after the first beside it | writing a message | 16:14:10 | 16:15:34 | **85 s** |
| 2026-09-25, Wreckfest 2's settings window | the machine used throughout | 16:24:54 | 16:24:59 | **5.2 s** |
| 2026-09-25, Starfield, after Wreckfest 2 beside it | the same | 16:30:33 | 16:30:52 | **19 s** |

The Battlefield 6 row's exit is read from the EA anti-cheat's file-system
filter unloading (`Microsoft-Windows-FilterManager` event 1), the rows
after it from `presence-probe watch`, which sees a process gone within
200 ms; they come from [Lot 9](09-robustness.md)'s runs of two games back
to back. The maintainer was at the keyboard throughout all of them, idle
two or three seconds at most.

**Measured:** Windows does not release its "a game is running" signal while
nobody touches the machine, and releases it some twenty seconds after the
first keyboard or mouse input that follows the game's exit. The screen is
not the trigger — it stayed on in A and C — and the party chat changes
nothing. The watcher's own share was its two-second `stop_delay` every time.
*Corrected 2026-09-25, afternoon:* the twenty seconds do not hold. With the
maintainer at the keyboard, the rows from American Truck Simulator on range
from 0.6 s to 85 s, and the foreground window at the release — Steam,
Claude, Explorer — does not sort the fast from the slow. What stands is the
first half: never while nobody touches the machine. What decides the time
once someone does is not known.

**Inferred, not measured:** that the delays of 52 seconds and two minutes
recorded in [Lot 1](01-console-watcher.md) were the time until the
maintainer next touched the machine; that the rule holds for every title
and every machine — only GTA Online, on the maintainer's machine, was
measured.

## What Windows documents

The [presence writer's page](https://learn.microsoft.com/en-us/windows/win32/devnotes/gamebar-presencewriter)
says Windows calls the writer's `UpdatePresence` with one of three events:
the game got focus, lost focus, or was closed — `AppClose`. That last one is
the documented verdict that a game has ended. It goes to the registered
writer, and only Windows may register one: the key is owned by
TrustedInstaller ([Detection](00-detection.md)). The page says nothing of
when Windows lets the writer go after it.

What this program observes is the writer's process, which ends within
20 ms of the last reference to it being released
([Detection](00-detection.md)). So the reference is what waits for input;
which Windows component holds it is not known. `GameBar.exe`,
`GameBarFTServer.exe` and the `BcastDVRUserService` were running on
2026-09-24, none suspended, while the maintainer was at the machine; what
they do while nobody is was not seen.

## Whether it is ours

For:

- The common end of an evening — quit, walk away — leaves the "game"
  commands in force for hours: fans at their game profile, a power plan
  left high. That is the opposite of what the program is for.
- Windows knows the game has gone: its process has ended, and the watcher
  itself notes it when the writer is finally released.
- Waiting on a game's own process is already how a game marked by hand
  ends ([Lot 15](15-marked-games.md)): a handle, no polling, nothing
  guessed.

Against:

- *Detection is Windows' own verdict* is the principle the program is
  built on, and this delay is Windows' behaviour, not a fault of ours.
- The only end signal within reach for a title Windows names is its
  process, and that process is known only through naming, which
  [Lot 3](03-game-naming.md) keeps out of detection on purpose: *it never
  feeds detection*.
- The 2026-09-10 decision in [Lot 1](01-console-watcher.md) weighed
  exactly this and kept the writer alone, for two risks: a game that
  restarts its own process mid-session — Forza did — and a name given to a
  satellite that exits while the game goes on.

## Options, none chosen

1. **Say it, and nothing more.** Done on 2026-09-25: How it works and
   Getting started now say the wait lasts until the next touch of the
   mouse or the keyboard. The evening case stays as it is. Reworded the
   same afternoon: the wait never ends while nobody touches the PC, and
   took from under a second to a minute and a half at the keyboard.
2. **End on the game's own exit, when the game is named.** The engine would
   wait on the named process as well as on the writer, and end the session
   once no process matching the title's Known Game List entry is left —
   after a grace period, so that a game restarting its own process keeps
   its session, and whatever the name, since the game itself still matches
   while it runs. An unnamed session keeps the writer alone. It rewrites
   Lot 3's rule and reopens the Lot 1 decision, and the two risks above are
   what it would have to be measured against.
3. **A Windows signal not found yet.** Microsoft's documentation read
   again first, for a documented signal of a game's end that does not wait
   for input — Game Mode, GameDVR, the Game Bar's own writes. None is known
   today.

A setting to choose between the writer and the process is not proposed:
a switch whose behaviour needs explaining is what *Strict and simple over
clever* argues against, unless the maintainer wants one.

## What would be measured before deciding, if the lot is taken

- How often, in real sessions, the named process ends while the writer
  stays and the game goes on — the restart and the satellite cases. The
  engine could write it to the log at `debug` without acting on it, over a
  few weeks of the maintainer's titles: Forza, Battlefield 6 and its
  anti-cheat launcher, Starfield.
- Whether the input rule holds for other titles and on the second machine,
  with the same protocol as A and B. At the keyboard, six titles measured
  on 2026-09-25 took 0.6 s to 85 s; A, hands off, has been run on GTA
  Online alone.
