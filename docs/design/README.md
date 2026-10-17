# Design record

Why the program is the way it is: decisions, measurements, what is open. For
using it, read [Getting started](../getting-started.md) and
[How it works](../how-it-works.md).

Work comes in numbered **lots**, each with a page and a "done when", closed
once verified in a real game session. The table says where each stands today;
the pages keep the history.

| Lot | Page | Today |
| --- | --- | --- |
| — | [Detection: why the Game Bar presence writer](00-detection.md) | the foundation, with the games marked by hand added by Lot 15 |
| 1 | [A console watcher that is correct and boring](01-console-watcher.md) | done 2026-09-10 |
| 2 | [Configuration, and how commands run](02-configuration.md) | done 2026-09-10 |
| 3 | [Naming the game](03-game-naming.md) | done 2026-09-10; the retry while undecided is Lot 9's |
| 4 | [Logs that speak to two readers](04-logging.md) | done 2026-09-10; one file a day since Lot 17 |
| 5 | [A Windows program with no window](05-windowless-watcher.md) | done; the logoff it could not cover is Lot 9's |
| 6 | [Notification area icon](06-notification-icon.md) | done 2026-09-15 |
| 7 | [Icon, tooltip and menu as one state](07-tray-state.md) | done 2026-09-15 |
| 8 | [Distribution](08-distribution.md) | done 2026-09-18; the second machine's proposals built 2026-09-25 |
| 9 | [Robustness](09-robustness.md) | partly done: session marker, faults, live reload, every way a session ends (2026-10-17); the refinement's retry not yet met in a game |
| 10 | [Configuration window](10-configuration-window.md) | proposed; set aside until a UX/UI analysis |
| 11 | [Documentation for the people who use it](11-user-documentation.md) | done 2026-09-14; cut for concision 2026-10-17 |
| 12 | [Editing the configuration without breaking it](12-editing-on-a-copy.md) | proposed |
| 13 | [Updating](13-updating.md) | done 2026-09-23 |
| 14 | [Release notes people can read](14-release-notes.md) | done 2026-09-18 |
| 15 | [Games Windows knows only from you](15-marked-games.md) | done 2026-09-23 |
| 16 | [What the watcher keeps](16-footprint.md) | done 2026-09-23; memory at rest to measure again against 0.3.0 |
| 17 | [A log that turns over each day](17-log-rotation.md) | done 2026-09-26; its last checks seen 2026-10-17 |
| 18 | [When the game is gone and nobody is there](18-game-gone-nobody-there.md) | proposed, not decided |
| 19 | [Starting from Windows' Startup list](19-startup-list.md) | proposed; a spike first |
| 20 | [The build chain, official and kept current](20-build-chain.md) | built 2026-10-17; the first wait and the first attested release to see |
| 21 | [Moving to windows-rs 0.100](21-windows-rs-0100.md) | proposed; waiting for `windows` 0.100 |

Logging came before the icon, so the icon was written in the final log
vocabulary; the windowless build came before it too, so the window and message
loop it hangs off were proved with nothing drawn. The numbers moved twice
early on; pages and commits use the final ones.

## Non-goals

- **No integration with a particular tool.** The program runs executables.
  FanControl lives in its recipe.
- **No Windows service.** Session 0 cannot see the desktop.
- **No list of games, no heuristics.** Detection is Windows' verdict.
- **No telemetry, no unasked network.** One connection: *Check for updates*,
  on a click ([Lot 13](13-updating.md)).
- **No elevation.** Programs needing administrator rights go through a
  scheduled task ([Lot 1](01-console-watcher.md)).
- **No configuration window before Lot 10.**
- **No version key in `config.toml` or the session marker.** Unknown keys are
  refused and missing ones defaulted, and the marker is read leniently; a
  breaking rename would be a major version with its migration.

## Assumptions still to verify

| Assumption | Status |
| --- | --- |
| The presence writer starts for games only | Notepad was a clean negative; not proof for every application. |
| The writer never blinks mid-session | Held over every session so far, alt-tabs included; `presence-probe watch` samples at 100 ms, so a shorter dip could hide. |
| Naming covers the titles played | Every title played so far was named, by path, package family or the GPU. |
