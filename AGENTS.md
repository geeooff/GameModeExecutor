# Working in this repository

GameModeExecutor is a Rust program for Windows that runs configured
executables when a game starts and stops. Detection is Windows' own verdict
— the Game Bar presence writer's lifetime, or a process marked as a game in
the Game Bar — never a list of ours. It runs unelevated, connects to nothing
unasked, and shows only a notification area icon.

This file says how to work here. The *why* is in
[`docs/design/`](docs/design/README.md), the exact behaviour in
[`docs/reference.md`](docs/reference.md).

## Principles

- **OS-native over heuristics.** Read from Windows what Windows knows: no
  allow-lists, no scanning, no guessing what a game is. Wait on a handle
  rather than poll; prefer a documented API to a workaround, and a
  workaround that fails visibly and harmlessly to one that fails silently.
  The one undocumented call, `SetPreferredAppMode`, is justified in
  `docs/design/06-notification-icon.md`.
- **Microsoft's documentation first, then measure.** Read and cite what
  Microsoft documents; spike only what it leaves open, logging what the
  system actually does, then decide. Nothing works until a real game session
  has exercised it, and a reader of real data is not done until it has read
  the real data.
- **Cheap at rest, silent in game.** The idle look is the only timer and
  stays the cheapest thing the program does; in a game the watcher waits on
  a handle. A change to either is measured on the installed process against
  the last release — processor, private memory, handles; `presence-probe
  cost`, `footprint`, `menu-cost` — and the figures go in the record and the
  changelog. Measure what a call leaves, not only what it takes: PDH and the
  shell kept what they loaded (`docs/design/16-footprint.md`).
- **Strict and simple over clever.** An unambiguous state beats a fallback
  that needs explaining; argue for a fallback only if it protects something
  concrete.
- **Discreet.** No dialogs, windows or sounds: the icon, its tooltip and its
  menu are the interface. A silent notification only answers a click, or
  says the configuration cannot be used and then that it can again. The log
  is the rest.
- **No elevation, service, telemetry, or unasked network.** The one
  connection is *Check for updates*, on a click
  (`docs/design/13-updating.md`). Programs needing administrator rights go
  through a scheduled task the user registers once.
- **Microsoft libraries for the platform**: the `windows` crate, the SDK's
  `rc.exe`; no third-party tray, icon or installer crate.

## How work is organised

- Work comes in numbered **lots**, each with a page in `docs/design/` and a
  "done when"; proposed before built, closed only when verified in the
  field. Propose new lots or a new order with the reasoning; the maintainer
  decides.
- **Explicit approval, every time:** pushing to the repository, publishing a
  release, changing the scheduled tasks or the configuration on the
  maintainer's machine, rewriting `main`'s history.
- **A branch's history is yours to rewrite until it reaches `main`**, pushed
  pull request branches included (force-push allowed), and you should:
  before a push, reduce a lot to its logical commits. A fix, a second
  attempt or a page touched again for the same purpose goes into the commit
  it completes.
- **A change the user sees** updates `docs/getting-started.md` and
  `docs/how-it-works.md` in the same commit. A documented promise that a
  measurement contradicts is corrected in place, dated, not deleted.
- **User pages are short** — the README, *Getting started*, *How it works*,
  the reference, the changelog: what the person sees and does, a dated
  one-line correction, a link to the lot page when the reasoning is worth a
  click. A long page opens with a *TL;DR*. Measurements, tables and
  reasoning go in `docs/design/`, complete and clear.

## Conventions

- The repository is in English, in natural sentence case: no `ALL CAPS`
  categories, no `camelCase` in prose. Conversation with the maintainer is
  in French.
- **Words in their exact sense**, as the source tool names them: FanControl
  saves *configurations*, not profiles; *the zip* or *a hand-installed
  copy*, never *portable*. A wrong word is retired everywhere in one commit,
  with the reason in the design record.
- **Log lines follow the contract in `docs/reference.md`.** `info` is for
  detection, the watcher's start and stop, what the setup commands did, and
  each update step; otherwise `debug`, `warn` for a degradation, `error`
  when the user is needed. The message is the sentence, the fields are the
  annex, and every call names a `target:` — a test enforces it.
- **The tray renders state and holds no rule.** It asks the objects that own
  the rules — the engine's session, `update`'s phase, the supervisor's
  verdict — what to draw and what a click means.
- **The setup commands are a contract with three callers.** `stop`, `init`,
  `install-task` and `uninstall-task` are sequenced by the package
  (`scripts/msi.ps1`), by the zip's after-exit shell in `update`, and by
  `purge`. A change to any of them is verified on all three paths.
- Module doc comments carry the rules a module is shaped by: read them
  first, update them with the rule.
- **Every `unsafe` block and `unsafe impl` has a `// SAFETY:` comment**
  saying why it is sound — which pointer outlives what, which size bounds
  which write. Clippy denies a missing one.
- **Tests.** Pure logic gets a unit test; Win32 behaviour is verified by hand
  and dated in the design record. The engine reads the OS only through
  `sensor::Sensor` (`engine/tests.rs` scripts whole sessions; a loop change
  gets a scenario), the updater the network only through
  `update::feed::Feed`. Split a rule from the system call beside it by
  passing the call in, as `sensor::sighting_among` does.
- **No test calls an external host**, ignored or not. The network path is
  measured by hand with `gamemode-executor update --check`.
- Commits: an imperative subject, a short body saying what changed and why,
  and a `Co-Authored-By` trailer for the agent that co-wrote it.

## Workflow

```powershell
.\scripts\build.ps1 test       # fmt, clippy -D warnings, tests, shipped config.toml files, doc links
.\scripts\build.ps1 build      # + release build, PE subsystem check
.\scripts\build.ps1 release    # + clean tree, stamped commit, dist\ zips, MSI built and validated
```

- Run `test` before every commit and read its result; never commit in the
  same block as a check that may fail.
- `release` needs a clean tree: the binaries carry their commit, and
  `--version` links to that commit's `docs/getting-started.md`. Build it from
  the commit with the final documentation.
- **Publishing a release** is a tag, approved like any push. Bump `version`
  in `Cargo.toml`, turn `[Unreleased]` in `CHANGELOG.md` into
  `## [x.y.z] - YYYY-MM-DD` dated the release commit, merge, tag `vX.Y.Z`,
  push the tag. The workflow publishes the installer, the zips and their
  checksums with that section as the notes, and refuses a version the
  changelog does not carry, dated. Versions follow
  `docs/design/08-distribution.md`; 1.0.0 waits for its criteria.
- Before each release, check the documentation's external links by hand —
  FanControl's site and releases above all. `build.ps1` checks only the
  repository's own links.
- **The changelog is the agent's to write**, read by the maintainer as a
  diff. Each line is something the user sees or does, under *Added*,
  *Changed*, *Fixed* or *Removed* — never code, modules or commits, which
  the release page lists under *For the curious*. One line per change for
  them, however many commits; none for tests, refactors, the design record
  or this file. Written in `[Unreleased]` as work lands, at the latest in
  the release commit. A published section is history, corrected only for
  an error of fact.
- Tests that read this machine — the Known Game List, the games marked by
  hand, Microsoft's list, the Game Bar registration, the real sensor — are
  `#[ignore]`d with the reason and run by the script when `CI` is not set.
  CI stays green on a stock runner. One more runs only by name, since it
  starts the real presence writer and an installed watcher would run the
  user's commands: `cargo test -- --ignored a_real_activation_drives_a_session`.
- **Verify as the user runs it:** `gamemode-executor stop`, copy the two
  release executables over the installed ones, `gamemode-executor
  install-task`, and read the log at `debug` through a real game. A package
  change: uninstall from *Programs and Features*, install the new `dist\`
  package — the product code is fixed per version. Never restart the
  watcher while a game runs: that fires the stop commands.

## Pitfalls that have already cost time

- Shell substitutions and heredocs eat backslashes:
  `GameModeExecutor\FanControl` became `GameModeExecutorFanControl`,
  `target\release` a carriage return. Edit with a tool that takes literal
  strings, and grep the result.
- A shell started by a packaged host — the Claude desktop app — may read
  `%APPDATA%` through the package's private copy. What the watcher runs is
  in its log; ask the maintainer for the real file.
- Windows' registry change notification never arrives for the Game Bar's
  writes to its game list, eight ways measured
  (`docs/design/15-marked-games.md`).
- PowerShell: `$LASTEXITCODE` is set by native commands only, so use
  `try { … -ErrorAction Stop } catch`. `Select-Object -First N` stops the
  upstream pipeline mid-run: never after `build.ps1`, where a truncated
  checklist once reported OK — read the whole output, or `release OK`. In a
  non-interactive shell, `Remove-Item` without `-Recurse` on a folder that is
  not empty errors with exit code 1: test for emptiness first, as `purge`'s
  `remove_if_empty` does.
- The task templates in `docs/recipes/` are UTF-16 with a BOM and CRLF, and
  carry placeholders the release checks; keep that encoding.
- `TrackPopupMenuEx` is modal and re-enters the window procedure: hold no
  `RefCell` borrow across it, or across any call that can show UI. The
  watcher never calls the shell; menu entries go through the `open` helper,
  `src/open.rs`.
- `FindWindow` cannot find a window class registered by another process; use
  `EnumWindows`.
- A process started after `WM_QUERYENDSESSION` dies with
  `STATUS_DLL_INIT_FAILED`: nothing runs at logoff, so the session marker
  runs the stop commands at the next start.
- `.git/HEAD` does not change on commit; `build.rs` watches the ref it names
  and `packed-refs`, or the stamp goes stale.

## Working with the maintainer

- Read what is linked before designing from it.
- Give a recommendation, not a survey; say what was measured and what was
  inferred.
- Say who saw it: the log, a probe or the maintainer's eyes. The maintainer's
  eyes outrank a probe's negative — a probe once missed a console that
  flashed twice.
- Report outcomes exactly — a failed test, a skipped step, a claim that
  proved wrong — and correct in place, dated.
- Machine-specific facts belong in `CLAUDE.local.md`, never here.
