# Lot 3 — Naming the game

**Goal.** Name the running game from the registry, while being explicit that
failing to name it is not a failure of the program.

**Done when:** a session with an unrecognised game runs the commands normally
and leaves a log line that unambiguously says no entry in Windows' Known Game
List matched. Closed 2026-09-10.

- [x] Read Windows' Known Game List from `HKCU\System\GameConfigStore\Children`
- [x] Win32 titles matched by `MatchedExeFullPath` and `ExeParentDirectory`
- [x] Packaged Store and Game Pass titles matched by package family name, wherever installed
- [x] Generic directory names (`x64` and friends) ignored rather than trusted
- [x] `check <path>` to interrogate the list by hand
- [x] The unmatched case logged plainly at `info`, not only at `debug`
- [x] The real game told from its satellites by GPU rendering load
- [x] Researched what other products do before building anything

Naming feeds the log and the action placeholders. It never feeds detection.

## Two families in the registry

The entries under `HKCU\System\GameConfigStore\Children` fall into two
families:

- `Type = 1`, Win32 titles, identified by `MatchedExeFullPath` or
  `ExeParentDirectory`. The second field is inconsistent — sometimes a full
  path, sometimes a bare folder name, and one real entry is just `x64` — so
  bare names that are too generic are ignored rather than trusted.
- `Type = 2`, packaged Store and Game Pass titles, which have no executable
  path at all and are identified by `UtmItemId`, shaped
  `P~<PackageFamilyName>!<AppId>`. Starfield is one of these, so path matching
  alone would never have named it; they are matched by asking the running
  process for its package family name.

When nothing matches, the actions still run, with the placeholders empty.

## Why naming is hard, and why it stays best effort

A game is not one process. It is an installer stub for dependencies, a splash
screen, a third-party launcher, an anti-cheat service, and somewhere among them
the executable a player would name. They share an install folder or a package
family, so they all match, and the one that matches first is not necessarily
the game.

Three early sessions, three different ways of getting it wrong: Starfield was
named after `gamelaunchhelper.exe`, Battlefield 6 after
`EAAntiCheat.GameServiceLauncher.exe` matched through its parent directory, and
Forza kept a process id that a mid-session restart had replaced.

Telling the real game apart means observing which process actually does the
work. That is the kind of guessing the project set out to avoid — so it is
allowed in exactly one narrow form, and its absence is a normal outcome.

## The refinement

A little way into the session (`identify_after`, 20 s by default), the
candidates the Known Game List produced are ranked by their share of the 3D
GPU engine over a one-second sample. The one rendering wins. The rule never
promotes a process the list did not match; a single candidate that is the
name in use, or the GPU confirming it, means "keep the current name", and
the log says which. *Corrected 2026-09-25:* this said "once", and that
unreadable counters and a loading screen kept the name too. Those, and no
match at all, are no answer rather than a verdict, and the question is asked
again at the same interval, six times at most — [Lot 9](09-robustness.md).

**One match left is not nothing to say.** Measured 2026-09-15 on Battlefield 6:
the EA anti-cheat *launcher* matches the install folder, starts before the
game and wins the first identify, then exits into a service that does not
match — leaving `bf6.exe` as the only candidate. The refinement bailed out on
"fewer than two candidates" and kept the name of a process that no longer
existed for the whole session. Now, when the process being named has gone and
one candidate is left, the survivor is adopted without any measurement; while
the named process is still alive, a single other candidate is ambiguous and the
name in use is kept.

The rename was first seen on screen on 2026-09-16, through the GPU path:

```
00:06:51.505  Game detected: EAAntiCheat.GameServiceLauncher.exe
00:07:12.850  Game identified more precisely: bf6.exe (75% of the rendering)
```

and the survivor rule the same night, on Starfield of all titles:

```
00:16:51.841  Game detected: gamelaunchhelper.exe   matched_by="package family"
00:17:11.964  Game identified more precisely: Starfield.exe (the only match left)
```

The same title had named itself directly the day before. Which process wins
the first identify is a race, and the survivor rule is what makes losing it
harmless.

**The margin is thin, and known.** Ten seconds before the attempt in the BF6
session, `status` read 0.0 % for every candidate; ten seconds later, 75 %. The
single timed attempt landed just inside the window that makes it work. That is
a margin, not a calibration — [Lot 9](09-robustness.md) has the design that
would replace the timer. *Since 2026-09-25* an attempt that reads nothing
rendering is followed by another, so a loading screen costs twenty seconds
rather than the name.

## Packaged titles are matched wherever they are installed

An earlier version only asked a process for its package family name when its
image path sat under `WindowsApps`. The Store lets a game be installed
anywhere; the `WindowsApps` entry is then a junction Windows resolves, so the
running process reports the real path and never looked packaged. The filter was
an optimisation that quietly encoded an assumption about install locations.
Both questions are now asked on a single process handle, which is also cheaper
than the two opens it replaced.

PowerShell disagrees with the program about such a process's path:
`Process.Path` goes through `GetModuleFileNameEx` and reports the `WindowsApps`
path, while `QueryFullProcessImageNameW` with `PROCESS_NAME_WIN32` resolves the
junction. Worth knowing before a PowerShell reading looks like a contradiction.
