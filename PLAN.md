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
- [ ] File logging on by default, not only when `log_dir` is set
- [ ] Log lines that state plainly: game detected, game no longer detected
- [ ] Real FanControl actions, once a `Game.json` profile exists
- [ ] One real game session, log read and checked
- [ ] Logon task installed and confirmed across a reboot

Done when: a real game session drives the configured commands, start and stop,
started automatically at logon, with a log file that shows what happened and no
known defects.

State:

- FanControl **v275, portable**, at
  `C:\Users\geoff\OneDrive\Applications\FanControl`, with one profile,
  `Configurations\Quiet.json`. `Game.json` does not exist yet.
- The CLI assumption held: `-c` / `--config` takes `yourConfig.json`, extension
  included, and *switches an already-running instance* rather than starting a
  second one. No correction needed to the README.
- Working config at `%APPDATA%\GameModeExecutor\config.toml`, currently running
  harmless placeholder actions (a line in `events.log`, plus a rising or falling
  beep), with the FanControl actions written out and commented, ready to swap in.
- Measured 2026-09-09: start fired 2.00 s after the writer appeared (the 2 s idle
  poll, worst case), stop 5.01 s after it exited (`stop_delay`).

Notes:

- File logging is currently opt-in through `log_dir`, and only a hidden instance
  gets a default path. For this lot it should just always write a log file.
- Everything after this lot is polish on an unproven product until a real game
  session has driven real commands.

---

## Lot 2 — JSON configuration, parallel or serial commands · committed · `[ ]`

Goal: a configuration format that is easier to hand-edit than TOML, and control
over how several commands run for one event.

**Decided: JSONC**, so the shipped template can carry comments — and in
particular explain that backslashes must be doubled.

- [ ] Move the configuration to JSONC
- [ ] Several commands per event, at game start and at game stop
- [ ] Per-event execution mode: parallel or serial
- [ ] Validation pass with errors that point at a line and column
- [ ] A dedicated exit code when the configuration is invalid
- [ ] Documented template explaining every field, doubled backslashes included
- [ ] Migrate the shipped example and the `init` template
- [ ] Decide the fate of existing TOML files (read both for a while, or convert once)

Done when: a JSONC config with several commands per event runs them in the
configured order or concurrently; an invalid config makes the program refuse to
start, say where the problem is, and exit with the dedicated code.

### Comments without a dependency

Comments can be supported with **no new crate**: blank `//` and `/* */` with
spaces of the same length before handing the text to `serde_json`. Because the
byte offsets are preserved, the line and column in a parse error still point at
the real position in the user's file — which is the whole point of the
validation step.

Trailing commas cannot be handled that way and would need a real parser. Not
worth it for a first pass; the template will simply not use them.

### Exit codes

A small documented set, to be settled in this lot. Note that `clap` already
returns `2` for command-line misuse, so that value is spoken for.

| Code | Meaning |
| --- | --- |
| 0 | success |
| 2 | command line misuse (clap) |
| 3 | configuration file not found |
| 4 | configuration invalid: syntax or validation |
| 5 | another instance is already running |
| 1 | anything else |

Notes:

- Serial mode already exists in substance through the per-action `wait` flag;
  this lot turns it into an explicit per-event mode rather than a per-action
  detail.
- Parallel mode needs a defined answer for failures: one command failing must not
  prevent the others, and the log has to make clear which one failed.
- **Comments are disposable.** Decided: anything that rewrites the file may lose
  the user's own comments, and that is accepted. The configuration is a
  convenience, not a document. No comment-preserving serializer is needed —
  `serde_json` plus a regenerated header is enough.
- One consequence worth honouring anyway: a rewrite should **re-emit the
  documented header**, so the file does not decay into bare JSON after its first
  save and stop explaining the doubled backslashes.

---

## Lot 3 — Clean game naming · committed · `[~]`

Goal: name the running game from the registry, while being explicit that failing
to name it is not a failure of the program.

- [x] Read Windows' Known Game List from `HKCU\System\GameConfigStore\Children`
- [x] Win32 titles matched by `MatchedExeFullPath` and `ExeParentDirectory`
- [x] Packaged Store and Game Pass titles matched by package family name
- [x] Generic directory names (`x64` and friends) ignored rather than trusted
- [x] `check <path>` to interrogate the list by hand
- [ ] Say so plainly when no known game matched, in the log and on screen, at
      normal level rather than only in debug
- [ ] Make the placeholders behave predictably when the name is unknown
- [ ] Document that this is naming only, never detection

Done when: a session with an unrecognised game runs the commands normally and
leaves a log line that unambiguously says no entry in Windows' known game list
matched.

Notes:

- Most of this lot already exists; it was built alongside detection. What remains
  is the messaging, which is the part that matters for the stated goal.
- Today the unmatched case logs `game started (unidentified)` at info and the
  detail only at debug. That is close but not explicit enough about *why*.
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

Settled already: saving may lose the user's comments. It should still re-emit the
documented header from Lot 2, so the file keeps explaining itself.

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
| FanControl switches profiles with `-c <profile>.json` | Confirmed by the official docs, which add that it switches a running instance. Not yet exercised against the binary. |
| The presence writer is activated for games only | Notepad was a clean negative control; not proof for every application |
| Naming covers the titles actually played | Two of two named, once packaged titles were supported |
| The writer never blinks mid-session | Holds over two sessions including alt-tabs; `watch` polls at 100 ms, so a sub-100 ms dip could hide |

---

## Journal

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
