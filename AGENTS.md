# Working in this repository

GameModeExecutor is a Rust program for Windows that runs configured
executables when a game starts and stops. Detection is Windows' own verdict —
the lifetime of the Game Bar presence writer process — never a list of games.
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
- **Measure before deciding.** When the documentation leaves the deciding
  question open, build the smallest thing that logs what the system actually
  does, then decide. Several early designs here were wrong until measured;
  the design record keeps the numbers. Do not report a mechanism as working
  until a real game session has exercised it.
- **Strict and simple over clever.** An unambiguous state ("it is off, fix the
  file") beats a fallback whose behaviour needs explaining. Put the strict
  option first and argue for a fallback only if it protects something
  concrete.
- **Discreet.** No dialogs, no windows, no sounds. The icon, its tooltip and
  its menu are the whole user interface; the log is the rest.
- **No elevation, no network, no service, no telemetry.** Recorded as
  non-goals in the design record with their reasons. Programs that need
  administrator rights are reached through a scheduled task the user registers
  once, never by elevating the watcher.
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
  reserved for detection, the watcher's own start and stop, and what the
  setup commands did to the machine; everything else is `debug` unless it is
  a degradation (`warn`) or needs the user (`error`). The message is the sentence, the fields are the technical annex,
  and every call names a `target:` — a test fails the build otherwise.
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
  whole sessions; a change to the loop gets a scenario there.
- Commit messages: an imperative subject, a short body saying what changed
  and why, and a `Co-Authored-By` trailer for the agent that co-wrote it. The
  collaboration is not hidden.
- FanControl is mentioned only in its recipe. Everywhere else the program
  "runs executables".

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
any push: bump `version` in `Cargo.toml` in the release commit, merge it,
tag that commit `vX.Y.Z`, push the tag. The release workflow runs the same
script on a runner and publishes the installer, the zip and their checksums.
Versions follow `docs/design/08-distribution.md`: the number moves only in
a release commit, and 1.0.0 waits for the criteria written there.

Three tests read this machine's registry — the Known Game List, the Game Bar
registration, the real sensor — which a GitHub-hosted Windows Server runner
does not have. They are `#[ignore]`d with that reason and the script runs
them when `CI` is not set. CI must stay green on a stock runner: a test that
needs a real Windows client, a GPU or a game says so with `#[ignore]`.

One more is run only by name, because it starts Windows' presence writer for
real and an installed watcher on the same machine reacts by running the
user's commands. It is the whole chain, end to end, with no game:

```powershell
cargo test -- --ignored a_real_activation_drives_a_session
```

Verifying a change means running it as the user does: deploy the two
executables to the install folder, restart the logon task
(`schtasks /Run /TN "GameModeExecutor\Watcher"`), and read the log at `debug`
through a real game session. Restarting the watcher while a game is running
fires the stop commands; do not.

## Pitfalls that have already cost time

- `sed` and shell substitutions eat backslashes: `GameModeExecutor\FanControl`
  becomes `GameModeExecutorFanControl` and `validate` accepts it. Edit files
  with a tool that takes literal strings, and grep the result.
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
  `RefCell` borrow across it, or across `ShellExecuteW`.
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
