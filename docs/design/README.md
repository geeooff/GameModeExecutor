# Design record

Why the program is the way it is: the decisions, the measurements behind
them, and what is still open. Written for someone evaluating or changing the
code — the pages for people who *use* the program are
[Getting started](../getting-started.md) and [How it works](../how-it-works.md).

Work is organised in numbered **lots**. A lot is a deliverable with a stated
"done when", taken in order, closed when it is verified against a real game
session rather than when the code compiles. Each has its own page.

| Lot | Page | Status |
| --- | --- | --- |
| — | [Detection: why the Game Bar presence writer](00-detection.md) | the foundation |
| 1 | [A console watcher that is correct and boring](01-console-watcher.md) | done |
| 2 | [Configuration, and how commands run](02-configuration.md) | done |
| 3 | [Naming the game](03-game-naming.md) | done |
| 4 | [Logs that speak to two readers](04-logging.md) | done |
| 5 | [A Windows program with no window](05-windowless-watcher.md) | done |
| 6 | [Notification area icon](06-notification-icon.md) | done |
| 7 | [Icon, tooltip and menu as one state](07-tray-state.md) | done |
| 8 | [Distribution](08-distribution.md) | done |
| 9 | [Robustness](09-robustness.md) | partly done; faults and live reload built, closing on a real session |
| 10 | [Configuration window](10-configuration-window.md) | proposed |
| 11 | [Documentation for the people who use it](11-user-documentation.md) | done |
| 12 | [Editing the configuration without breaking it](12-editing-on-a-copy.md) | proposed |
| 13 | [Updating](13-updating.md) | shipped in 0.2.0; closes on the first update from it |
| 14 | [Release notes people can read](14-release-notes.md) | done |

**Dependency order:** 1 → 2 → 4 → 5 → 6 → 7, with 3 independent and 7 needing
both 3 and 6. Logging sits before the icon deliberately — the icon logs too,
and writing it in the finished vocabulary is cheaper than converting it after.
The windowless build sits before the icon for the same reason the other way
round: the icon hangs off a window and a message loop, and those were proved
with nothing on screen before anything was drawn on them.

The numbering moved twice while the lots were being taken — logging became
Lot 4 and the tray icon lot was split in two. The pages and the commit
messages both use the final numbers. One lot is deliberately unnumbered: a
portable mode proper, everything in one folder — see
[Lot 8](08-distribution.md) for why the zip is not that — waits for a need.

**Standing rule:** any lot that changes what the user sees updates
`getting-started.md` and `how-it-works.md` in the same commit. Documentation
that describes a program that no longer exists is worse than none.

## Non-goals

Recorded so they stop coming back.

- **No FanControl-specific integration.** The program runs executables; that
  is all. FanControl is the example that drove the project and lives in its
  recipe and nowhere else.
- **No Windows service.** Session 0 cannot see the desktop or the user's
  applications.
- **No allow-list of game executables, and no heuristics that guess at what a
  game is.** Detection is Windows' verdict, read from Windows.
- **No telemetry, and no network access the user did not ask for.** The one
  connection the program opens is *Check for updates*, on a click, and
  [Lot 13](13-updating.md) says what it sends and to whom. Nothing is ever
  polled, and an opt-in check at start, if it ever comes, defaults to off.
- **No elevation.** The watcher runs as the user, on purpose. Programs that
  need administrator rights are reached through a scheduled task, never by
  elevating the watcher — see [Lot 1](01-console-watcher.md).
- **No configuration GUI before Lot 10.** Until then the menu opens the file
  and the user brings their own editor.
- **No version key in `config.toml` or the session marker.** The marker's
  contract is its presence; its contents are informational and parsed
  leniently, so no future format can lose anything. For the configuration,
  `deny_unknown_fields` on every table already refuses a file from a newer
  program and names the key, and serde defaults absorb a file from an older
  one. The one case a number would catch — a breaking rename — is a major
  version bump once public, with a migration written then against a real old
  format. Until there is something to migrate, the program's version is the
  format's version.

## Assumptions still to verify

| Assumption | Status |
| --- | --- |
| FanControl switches configurations with `-c <name>.json` | Settled. Unreachable directly because the binary requires elevation; bridged through a scheduled task, and the chain verified by reading the applied configuration back from FanControl's own `CACHE` file. Holds for the installer version running as a service too. |
| The presence writer is activated for games only | Notepad was a clean negative control; not proof for every application. |
| Naming covers the titles actually played | Named in the field on every title played so far: Farming Simulator 25 and Skyrim Special Edition by executable path, Starfield by package family, Battlefield 6 by its anti-cheat's parent directory then by the GPU. |
| The writer never blinks mid-session | Holds over sessions including alt-tabs; `presence-probe watch` polls at 100 ms, so a sub-100 ms dip could hide. |
