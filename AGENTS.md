# Working in this repository

GameModeExecutor is a Rust program for Windows that runs configured
executables when a game starts and stops. Detection is Windows' own verdict —
the lifetime of the Game Bar presence writer process, or of a process the
person marked as a game in the Game Bar — never a list of our own.
It runs unelevated, connects to nothing, and shows only a notification area
icon.

This file is for coding agents and for people. It says how to work here; the
*why* behind the code is in [`docs/design/`](docs/design/README.md), the
exact behaviour in [`docs/reference.md`](docs/reference.md).

## Principles

These decide most questions before they are asked.

- **OS-native over heuristics.** When Windows already knows something, read
  it from Windows. No allow-lists, no scanning, no guessing at what a game is.
  Prefer waiting on a handle to polling; prefer a documented API to a
  workaround, and a workaround that fails *visibly and harmlessly* to one that
  fails silently. `SetPreferredAppMode` is the one undocumented call in the
  program and `docs/design/06-notification-icon.md` says why it was let in.
- **Microsoft's documentation first, then measure.** Before designing on a
  Windows behaviour, read what Microsoft documents about it and cite it in
  the record; a spike answers only what the documentation leaves open --
  how soon a process id is reused, which the pages do not say. When the
  deciding question is open, build the smallest thing that logs what the
  system actually does, then decide. Several designs here were wrong until
  measured, and the design record keeps the numbers. Do not report a
  mechanism as working until a real game session has exercised it, and do
  not call a reader of real data done until it has read the real data:
  a parser of Microsoft's game list passed its own tests and missed the
  very title it was written for.
- **Cheap at rest, silent in game.** What players check first is what the
  program costs them. The idle look is the only timer the program has and
  must stay the cheapest thing it does; during a game the watcher waits on
  a handle and does nothing. A change to either is measured on the whole
  installed process against the last release -- processor over minutes,
  private memory, handles -- with `presence-probe cost`, `footprint` and
  `menu-cost` for the steps, and the figures go in the record and the
  changelog. A system library can keep what it loaded for the life of its
  caller, whatever its close function says: PDH and the shell did
  (`docs/design/16-footprint.md`). Measure what a call leaves, not only
  what it takes.
- **Strict and simple over clever.** An unambiguous state ("it is off, fix the
  file") beats a fallback whose behaviour needs explaining. Put the strict
  option first and argue for a fallback only if it protects something
  concrete.
- **Discreet.** No dialogs, no windows, no sounds. The icon, its tooltip and
  its menu are the whole user interface, plus a silent notification in two
  cases only: to answer something the user clicked -- a menu closes on a
  click, as every Windows menu does, and the answer has to reach them
  somewhere -- and to say that the configuration cannot be used, and then
  that it can again, the one state that needs them. The log is the rest.
- **No elevation, no service, no telemetry, and no network the user did
  not ask for.** Recorded as non-goals in the design record with their
  reasons. The one connection the program ever opens is *Check for updates*,
  on a click, and `docs/design/13-updating.md` says exactly what it sends.
  Programs that need administrator rights are reached through a scheduled
  task the user registers once, never by elevating the watcher.
- **Microsoft libraries only.** The `windows` crate for Win32, the Windows
  SDK's `rc.exe` for resources. No third-party tray, icon, or installer crate.

## How work is organised

Work is taken in numbered **lots**, each with a "done when" and its own page
under `docs/design/`. Lots are proposed there before they are built and
closed only when verified in the field. Do not add or reorder lots on your
own; propose, with the reasoning, and let the maintainer decide.

**Needs explicit approval, every time:** creating the public repository,
pushing to it, publishing a release, changing the scheduled tasks or the
configuration on the maintainer's machine, and any history rewrite.

**The standing rule:** a change to what the user sees updates
`docs/getting-started.md` and `docs/how-it-works.md` in the same commit. A
documented promise that a measurement contradicts is corrected in place, not
deleted.

## Conventions

- Everything in the repository is in English, in natural sentence case — no
  `ALL CAPS` categories, no `camelCase` in prose. Conversation with the
  maintainer is in French.
- **Log lines follow the contract in `docs/reference.md`.** `info` is
  reserved for detection, the watcher's own start and stop, what the setup
  commands did to the machine, and each step of an update; everything else
  is `debug` unless it is a degradation (`warn`) or needs the user
  (`error`). The message is the sentence, the fields are the technical
  annex, and every call names a `target:` — a test fails the build
  otherwise.
- **The tray renders state and holds no rule.** What the icon, the tooltip
  and the menu show comes from objects that own the rules — the engine's
  session, `update`'s phase, the supervisor's verdict on the configuration
  — and the tray asks them what to draw and which action a click means. A
  rule written in the menu code is in the wrong place and cannot be tested.
- **The setup commands are a contract with three callers.** `stop`, `init`,
  `install-task` and `uninstall-task` are sequenced by the package
  (`scripts/msi.ps1`), by the zip's after-exit shell in `update`, and by
  `purge`, each in the order its own mechanism allows. A change to any one
  of those commands, however small, is verified on all three paths before
  it is called done.
- Module-level doc comments carry the rules a module is shaped by (the tray's
  re-entrancy rule, the marker's location, the engine's callback). Read them
  before changing a module, and update them when the rule changes.
- **Every `unsafe` block and `unsafe impl` carries a `// SAFETY:` comment**
  saying why the call is sound -- which pointer outlives what, which size
  bounds which write, which handle is closed where. Clippy's
  `undocumented_unsafe_blocks` is set to deny in `Cargo.toml`, so a missing
  one fails the build. Most of the program is FFI into Win32; "it compiles"
  is not an argument there.
- Pure logic gets a unit test; Win32 behaviour gets verified by hand and the
  result written into the design record with its date. The engine reads the
  OS only through `sensor::Sensor`, and `engine/tests.rs` scripts one to run
  whole sessions; a change to the loop gets a scenario there. The updater
  reads the network only through `update::feed::Feed`, scripted the same
  way. A rule that sits next to a system call is split from it -- the call
  passed in as a function, as `sensor::sighting_among` takes the process
  path lookup -- so the rule is tested without Windows and the call stays
  a line.
- **No test calls an external host**, ignored or not: the script runs the
  ignored tests on every developer machine, and a test that needs GitHub
  is a test that fails with the Wi-Fi. The network path is measured by hand
  with `gamemode-executor update --check` and recorded with its date.
- Commit messages: an imperative subject, a short body saying what changed
  and why, and a `Co-Authored-By` trailer for the agent that co-wrote it. The
  collaboration is not hidden.

## Workflow

```powershell
.\scripts\build.ps1 test       # fmt, clippy -D warnings, tests, every shipped config.toml validated, every doc link resolved
.\scripts\build.ps1 build      # + release build, PE subsystem check
.\scripts\build.ps1 release    # + refuses a dirty tree, checks the stamped commit, zips into dist\, builds and validates the MSI
```

Run `test` before every commit and read its result — a `FAILED` scrolling
past a `git commit` in the same block has happened. `release` requires a clean
tree because the binaries carry the commit they were built from, and
`--version` links to that commit's `docs/getting-started.md`: build a release
from the commit that carries the final documentation, never before it.

**Publishing a release** is a tag, and the tag needs explicit approval like
any push: bump `version` in `Cargo.toml` in the release commit, turn the
`[Unreleased]` section of `CHANGELOG.md` into that version's -- headed
`## [x.y.z] - YYYY-MM-DD`, the date of that commit, so the file reads on
its own without the release page -- merge it,
tag that commit `vX.Y.Z`, push the tag. The release workflow runs the same
script on a runner and publishes the installer, the zip and their checksums,
with the changelog section as the notes; `build.ps1 release` and the
workflow both refuse a version the changelog does not carry, dated. Versions
follow `docs/design/08-distribution.md`: the number moves only in a release
commit, and 1.0.0 waits for the criteria written there.

Before each release, check by hand that the documentation's external links
still lead to live pages — FanControl's site and its releases, credited in
its recipe, above all. `build.ps1` resolves only the repository's own
links: a check that needs the network would fail with the Wi-Fi.

**The changelog is written here, by the agent, and read by the maintainer
as a diff.** The workflow cannot summarise, and the release commit is made
on the maintainer's machine anyway, with the agent present. Rules for a
section, so that two agents write it the same way:

- Every line is something the person running the program can see or do,
  in a sentence, under *Added*, *Changed*, *Fixed* or *Removed*. Never the
  code, the module or the commit; those are on the release page under
  *For the curious*, which the workflow writes from `git log`.
- One line per thing that changed for them, however many commits it took;
  no line for what changed only for the people working here — tests,
  refactors, the design record, this file.
- Written from the commits since the previous tag and the design pages
  they touched, in `[Unreleased]` as the work lands or at the latest in the
  release commit. A link to a lot page is allowed when the reasoning is
  worth a click; no line needs one to make sense.
- Once a version is published its section is history: corrected in place
  only for an error of fact, never rewritten for taste.

A few tests read this machine — the Known Game List, the games marked by
hand in it, Microsoft's own list file, the Game Bar registration, the real
sensor — which a GitHub-hosted Windows Server runner does not have. They are `#[ignore]`d with that reason and the script runs
them when `CI` is not set. CI must stay green on a stock runner: a test that
needs a real Windows client, a GPU or a game says so with `#[ignore]`.

One more is run only by name, because it starts Windows' presence writer for
real and an installed watcher on the same machine reacts by running the
user's commands. It is the whole chain, end to end, with no game:

```powershell
cargo test -- --ignored a_real_activation_drives_a_session
```

Verifying a change means running it as the user does: `gamemode-executor
stop`, copy the two release executables over the installed ones,
`gamemode-executor install-task`, and read the log at `debug` through a real
game session. A change to the package itself is verified by uninstalling
from *Programs and Features* and installing the new `dist\` package — the
product code is fixed per version, so the same version cannot install over
itself. Restarting the watcher while a game is running fires the stop
commands; do not.

## Pitfalls that have already cost time

- `sed` and shell substitutions eat backslashes: `GameModeExecutor\FanControl`
  becomes `GameModeExecutorFanControl` and `validate` accepts it. So do
  string literals in a script that writes a file -- `target\release` became a
  carriage return in a Python heredoc. Edit files with a tool that takes
  literal strings, and grep the result.
- A shell started by a packaged host -- the Claude desktop application is
  one -- may read `%APPDATA%` through the package's private copy: a file
  read there need not be the one the watcher reads, and a stale copy of the
  maintainer's configuration was quoted as theirs. What the watcher runs
  is in its log; ask the maintainer for the file itself.
- Windows' registry change notification never arrives for the Game Bar's
  writes to its game list, from an ordinary process, whichever way it is
  asked -- eight ways measured, `docs/design/15-marked-games.md`. Do not
  build on it again without the reason.
- In PowerShell, `$LASTEXITCODE` is set by native commands only; after a cmdlet
  it is stale. Use `try { … -ErrorAction Stop } catch`.
- `Select-Object -First N` **stops the upstream pipeline** once it has N
  objects, and a `.ps1` upstream is aborted mid-run with its last native exit
  code left standing. Never put it after `build.ps1`: a truncated checklist
  reported OK, and a stale `dist\` was deployed. Filter with `Select-String`
  and read the whole output, or read `release OK` at the end.
- The task templates in `docs/recipes/` are UTF-16 with a BOM and CRLF, as
  Task Scheduler exports them, and carry placeholders the release check
  verifies. Read and write them with that encoding.
- `TrackPopupMenuEx` is modal and re-enters the window procedure; never hold a
  `RefCell` borrow across it, or across any call that can show UI. The
  shell is no longer called from the watcher at all: the menu's entries go
  through the `open` helper, `src/open.rs`, and a new one should too.
- `FindWindow` cannot find a window whose class was registered by another
  process; use `EnumWindows`.
- A process started after `WM_QUERYENDSESSION` dies with
  `STATUS_DLL_INIT_FAILED`. Nothing can run a command at logoff; the session
  marker runs it at the next start instead.
- The refinement's single timed attempt is a known margin, not a calibration
  — `docs/design/09-robustness.md`.
- `.git/HEAD` does not change on commit; `build.rs` watches the ref it names
  and `packed-refs` too, or the stamp goes stale.

## Working with the maintainer

- Read what is linked before designing from it.
- Give a recommendation, not a survey; say plainly what was measured and what
  was inferred.
- Report outcomes exactly: a test that failed, a step that was skipped, a
  claim that turned out wrong. Corrections go in place, with the date.
- Machine-specific facts — install paths, where the maintainer's own tools
  live, how to reach files from inside a sandboxed shell — belong in
  `CLAUDE.local.md`, which is not committed, never in this file.
