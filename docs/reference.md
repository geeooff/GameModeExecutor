# Reference

The exact behaviour: commands, configuration, exit codes, the log contract,
building. For the shortest path to a working setup read
[Getting started](getting-started.md); for the reasoning behind any of this,
the [design record](design/).

## The executables

Self-contained, no runtime dependencies.

| Executable | What it is for |
| --- | --- |
| `gamemode-executor.exe` | Everything you type. A console program, so a shell waits for it, pipes work and exit codes come back. |
| `gamemode-executorw.exe` | The same commands with no console at all: it prints nothing, and a shell does not wait for it. What the logon task runs, and what the installer runs. |
| `presence-probe.exe` | Diagnostics, not shipped in the bundle. See [Detection](design/00-detection.md#the-instrument). |

The `w` suffix is the same convention as `python.exe` and `pythonw.exe`, for
the same reason: a program cannot be both a console and a windowless one in a
single file. Both share one library, so the commands they run are the same code.

## Commands

Both executables take the same command line. Type it into
`gamemode-executor.exe`: it answers where you can read it and a shell waits
for it. `gamemode-executorw.exe` accepts the same line but prints nothing and
nobody waits for it, which suits its two callers — the logon task, which
runs `run`, and the installer, which runs `stop`, `init`, `install-task`
and `uninstall-task`. What those commands do is written in the log either
way.

| Command | What it does |
| --- | --- |
| `run` | Watch and react, in this console. The default command. For an unattended instance use `gamemode-executorw.exe`. |
| `status` | The build, the presence writer's registration and whether it runs, the session marker, what the Known Game List holds and which running processes match it, ranked by GPU rendering share. |
| `check <path>` | Ask whether Windows knows a given executable as a game. |
| `trigger start\|stop` | Run one set of actions immediately, ignoring detection. Handy to test your commands. |
| `validate` | Parse and check the configuration. The command to script against: it returns 3 or 4 without starting anything. |
| `init [--force]` | Write the starter configuration file into `%APPDATA%\GameModeExecutor`. One that is already there is kept unless `--force`. The installer runs this. What happened is logged under `setup`. |
| `install-task [--delay 15s] [--force]` | Register a per-user logon task that runs `gamemode-executorw.exe` with no window, then start it now. A task already registered is kept unless `--force`. The configuration path is stored absolute. The installer runs this too. Logged under `setup`. |
| `uninstall-task` | Remove that task. No task is not an error. The installer runs this on an uninstall, not on an upgrade. Logged under `setup`. |
| `update [--check]` | Look for a newer release on GitHub; with `--check`, say so and stop. Otherwise download it, verify it against the release's `SHA256SUMS.txt` and install it the way the package or the zip's shell does — a running watcher hands its game session to the new one. The one command that connects to anything. Logged under `update`. |
| `stop [--handover]` | Stop the running watcher the way *Quit* in its menu does — mid-game, the stop commands run on the way out — and wait until it has gone. With `--handover` an open game session is left to the watcher that follows: the stop commands do not run, and the next start resumes the session with nothing run twice — for an update or an upgrade, where one follows within seconds. None running is not an error. The task is left alone; `install-task` starts it again. The installer runs this before removing (plain) or replacing (`--handover`) the executables. Logged under `setup`. |
| `purge [--yes]` | Remove every trace of the program: the logon task, the configuration, the log, the session marker, the executables. It lists what it will remove and asks; `--yes` is for scripts. Refuses while a game is running. See [Removing it](how-it-works.md#removing-it). |

Global options: `--config <PATH>`, `--log-level <LEVEL>`, `--version`.

Without `--config`, the file is looked up next to the executable first
(`config.toml`, a hand-installed copy), then in
`%APPDATA%\GameModeExecutor\config.toml`.

## Configuration

TOML. Every table refuses unknown keys, so a misspelt one is reported with its
line and column. [`config.example.toml`](../config.example.toml) documents
every field; the essentials:

```toml
[general]
stop_actions_on_exit = true   # run the stop commands if the watcher is stopped mid-game
log_level = "info"            # error | warn | info | debug | trace
#log_dir = 'C:\somewhere'     # default: %LOCALAPPDATA%\GameModeExecutor\logs

[detection]
poll_interval = "2s"          # how often to look for a game while idle
stop_delay = "2s"             # grace after the writer exits before the session ends
identify_after = "20s"        # when to ask the GPU which matched process is the game
gpu_sample = "1s"

[on_game_start]
mode = "series"               # or "parallel"

[[on_game_start.actions]]
name = "high performance power plan"
program = "powercfg.exe"
args = ["/setactive", "SCHEME_MIN"]
wait = true
timeout = "10s"

[on_game_stop]
mode = "series"

[[on_game_stop.actions]]
name = "balanced power plan"
program = "powercfg.exe"
args = ["/setactive", "SCHEME_BALANCED"]
```

Write Windows paths between single quotes: TOML takes those literally, so
backslashes need no doubling.

The watcher reads the file again whenever it changes — within about a
second of a save, the log says `Configuration reloaded` — and applies
everything but `log_dir`, which waits for the next start. A file it cannot
use disables it until one it can is saved: the icon turns red and its
menu's first line carries the reason; nothing runs meanwhile, and the exit
codes below are for the commands, since the watcher no longer exits over
the file.

### Actions

Each event has a mode and a list of commands. `series` runs each command after
the previous one has been waited for; `parallel` starts them all at once and
then waits. A command that fails to start is logged and never prevents the
others: an event is a set of independent side effects, not a pipeline.

| Field | Default | Meaning |
| --- | --- | --- |
| `name` | the program | the label used in the log |
| `program` | required | path or name of the executable |
| `args` | `[]` | one string per argument |
| `working_dir` | inherited | |
| `env` | `{}` | extra environment variables |
| `no_window` | `true` | start the process without a console window |
| `wait` | `false` | wait for it to exit before the next command, and record its exit status |
| `timeout` | none | give up waiting after this; the process is left running |
| `enabled` | `true` | |

These placeholders are substituted in `program`, `args`, `working_dir` and
`env`:

| Placeholder | Value |
| --- | --- |
| `{event}` | `game_start` or `game_stop` |
| `{process_name}` | e.g. `bf6.exe` |
| `{process_id}` | e.g. `12345` — on `game_stop` this is the id captured at the start, and that process is usually gone by then |
| `{process_path}` | the full image path, when readable |

Detection never depends on these. A game Windows tracks but does not name
leaves them empty, and everything still runs.

### Programs that require administrator rights

The watcher runs unelevated, on purpose, and Windows will not let it start a
program whose manifest requires administrator rights: the attempt fails with
error 740, `ERROR_ELEVATION_REQUIRED`. The bridge is a scheduled task per
command, registered once with *run with highest privileges*; triggering a task
needs no elevation and raises no prompt, so the action becomes
`schtasks /Run /TN <task>`.

The [FanControl recipe](recipes/fancontrol-fan-profiles/) has the full
treatment, the task templates and a script that registers them. The watcher is
never run elevated instead: the configuration names arbitrary programs, and an
elevated watcher would turn that file into a way to run code as administrator
with no prompt.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | success |
| 1 | anything else |
| 2 | command line misuse (returned by the argument parser) |
| 3 | configuration file not found |
| 4 | configuration invalid: syntax or validation |
| 5 | another instance is already running |

`3` and `4` come from `validate`, `status`, `trigger` and the setup commands,
which need a usable file. The watcher itself starts whatever the file says
and shows the fault in its icon instead.

## The log

`%LOCALAPPDATA%\GameModeExecutor\logs\gamemode-executor.log` unless `log_dir`
says otherwise, local timestamps, written synchronously. Run the watcher in a
terminal and the same lines appear there, coloured.

One log serves two readers, and `log_level` is the dial between them:

| Level | Written for | What it promises |
| --- | --- | --- |
| `error` | anyone | Something needs you. It names the file or command and what to check. |
| `warn` | technician | A degradation the program absorbed and carried on from — a command that exited non-zero, a marker it could not write. |
| `info` | anyone | The story of a session, in plain sentences. **The default.** |
| `debug` | technician | Why the program did what it did — and every line above, annotated. |
| `trace` | technician | Raw measurements. |

`info` is reserved for what the program is for: a game detected, named or
gone, the watcher starting or stopping, a session recovered at start, the
configuration reloaded — and what was done to this machine to set it up,
which is the same story one chapter earlier. Nothing else competes with
those lines. A configuration the watcher cannot use is an `error`, the one
line in the log that asks something of you: `The configuration cannot be
used, so nothing is watched until it is fixed: line 3: unknown field
'log_levl'`.

Each line is `time  LEVEL  category  message`, with the category one of
`watcher`, `game`, `commands`, `setup` or `update`:

```
2026-09-18 00:51:36.740  INFO  setup     Starter configuration written
2026-09-18 00:51:37.102  INFO  setup     Logon task registered: it starts the watcher at every logon, with no execution time limit
2026-09-10 17:51:02.433  INFO  watcher   GameModeExecutor 0.1.0 starting
2026-09-10 17:53:14.080  INFO  game      Game detected: bf6.exe
2026-09-10 17:58:41.833  INFO  game      Game no longer detected: bf6.exe
```

`setup` is written by `init`, `install-task`, `uninstall-task` and `stop`,
whether a person typed them or the installer ran them: a configuration
written, kept or replaced; a task registered, kept, replaced or removed; the
watcher started or stopped. Two processes then write the one file — `stop`
and the watcher it stops — and their lines interleave whole: the file is
opened for appending only, so Windows itself places each write at the end,
and a line is one write.
Those commands open the log where the watcher would — the configuration's
`log_dir` when a configuration can be read, the default location otherwise —
so a fresh install's first lines say what the installer did, and a machine
that misbehaves can be read back to the day it was set up.

`update` is every step of looking for, fetching and installing a newer
release — the only thing in the program that touches a network, so each
request is written down: `Checking for updates`, `0.1.0 is the latest
version` or `Update available: 0.2.0`, `Downloading 0.2.0 (1.4 MB)`,
`Downloaded and verified 0.2.0`, `Installing 0.2.0; the watcher stops now
and comes back on the new version`, then from the new watcher `Updated to
0.2.0`. A failure is a `warn` with the WinHTTP or Windows Installer code as
a field; `RUST_LOG=update=debug` adds every request and its status.

`debug` does not give a different log. It gives the same one annotated — the
technical detail rides along as fields rather than in lines of its own:

```
2026-09-10 17:53:14.080  INFO  game      Game detected: bf6.exe  pid=14552 matched_by="exe path"
2026-09-10 17:53:35.458  INFO  game      Game identified more precisely: bf6.exe (74% of the rendering)  pid=14552 rendering_share=73.8
2026-09-10 17:53:35.462 DEBUG  commands  Running the configured commands  count=2 mode="series"
```

A crash is logged as `FATAL:` at `error` level with the build and the location.
`RUST_LOG` overrides `log_level` when set and takes the usual `tracing` filter
syntax — `RUST_LOG=game=debug` for the detection lines alone.

## Files the watcher keeps

| File | Where | Nature |
| --- | --- | --- |
| Configuration | next to the executable, or `%APPDATA%\GameModeExecutor\config.toml` | yours; roams with the profile |
| Log | `%LOCALAPPDATA%\GameModeExecutor\logs\` | disposable |
| Session marker | `%LOCALAPPDATA%\GameModeExecutor\pending-stop-actions` | present while a game session is open; left behind by a logoff, shutdown, crash or handover, and settled at the next start — the session resumed if the game is still on, closed if it is gone. `status` reports it. |
| Fault marker | `%LOCALAPPDATA%\GameModeExecutor\configuration-fault` | present while the configuration cannot be used; removed when a usable one is read, which is how a watcher started on a file fixed meanwhile knows to say the fault is over |
| Logon task | `\GameModeExecutor\Watcher` in Task Scheduler | records the absolute path of the executable; removed with the package, kept through an upgrade |
| Updates | `%LOCALAPPDATA%\GameModeExecutor\updates\` | a downloaded release and the installer's log while an update runs; emptied when the next watcher starts, the log kept if the update failed |

## Building and releasing

Requires the Rust toolchain, stable, edition 2024. The Windows SDK's `rc.exe`
embeds the icon and the version block each executable carries — the one the
Properties dialog shows, with the commit in *File version*; without it the
build warns and continues.

```powershell
.\scripts\build.ps1            # test
.\scripts\build.ps1 build      # test, then a release build
.\scripts\build.ps1 release    # test, build, the zip archive and the installer in dist\
```

Each mode runs everything the one before it does. `test` is more than
`cargo test`:

| Step | Catches |
| --- | --- |
| `cargo fmt --check` | |
| `cargo clippy --all-targets -- -D warnings` | |
| `cargo test` | |
| every shipped `config.toml` through `validate` | a typo in a file people copy over their own |
| every documentation link resolved from its own file | a page that moved and a link that did not |

`build` adds the release build, then reads the **subsystem out of each PE
header**: a console program and a windowless one cannot be the same file, and
getting that backwards is invisible until someone sees a black window at logon.

A release proper is a tag. The version is bumped in `Cargo.toml` in the
release commit, `CHANGELOG.md` gains that version's section, that commit
is tagged `vX.Y.Z`, and pushing the tag makes the release workflow run
this same script on a GitHub runner, then publish the installer, the zip
and their SHA-256 checksums as a GitHub release, with the changelog section
as the notes and the commits since the previous tag below it. A version
the changelog does not carry is refused, by `release` here and by the
workflow. Nothing is built or uploaded by hand.

`release` refuses a dirty tree, checks the commit stamped into the binaries is
the commit being built, checks the version block each executable carries,
stages the bundle, zips it into `dist\`, builds the Windows Installer package
next to it and runs the SDK's every ICE over that package — a warning fails
the build — and refuses to finish if the archive names the building account
or if a task template has lost the placeholders that make it reusable. The
package is written by `scripts\msi.ps1` from Windows Installer's own
automation; nothing but the SDK is needed, and the validation tools are
unpacked from the SDK on first use.

**From VS Code:** `Ctrl+Shift+B` builds, and *Terminal → Run Task* offers the
same three plus two for driving an installed watcher — restart it, or follow
its log. They all call the script, so what runs in the editor is what runs in
a terminal.

## Limits

- Detection inherits Game Bar's coverage exactly: a title Windows has not
  recognised as a game triggers nothing — but it would not have got Game Mode
  either.
- The writer is activated for games, not for ordinary applications, but nothing
  guarantees a game is the only possible cause. `status` and the log show what
  fired.
- Only one instance runs per session; a second one exits immediately with code 5.
- The time between quitting a game and Windows releasing its signal is not
  predictable and not tunable — see [How it works](how-it-works.md).
