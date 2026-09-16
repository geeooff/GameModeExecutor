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
| `gamemode-executorw.exe` | Watching, and nothing else. No console at all — this is what the logon task runs. |
| `presence-probe.exe` | Diagnostics, not shipped in the bundle. See [Detection](design/00-detection.md#the-instrument). |

The `w` suffix is the same convention as `python.exe` and `pythonw.exe`, for
the same reason: a program cannot be both a console and a windowless one in a
single file. Both share one library, so the watcher they run is the same code.

## Commands

All belong to `gamemode-executor.exe`. `gamemode-executorw.exe` takes only
`--config` and `--log-level`, and watches.

| Command | What it does |
| --- | --- |
| `run` | Watch and react, in this console. The default command. For an unattended instance use `gamemode-executorw.exe`. |
| `status` | The build, the presence writer's registration and whether it runs, the session marker, what the Known Game List holds and which running processes match it, ranked by GPU rendering share. |
| `check <path>` | Ask whether Windows knows a given executable as a game. |
| `trigger start\|stop` | Run one set of actions immediately, ignoring detection. Handy to test your commands. |
| `validate` | Parse and check the configuration. The command to script against: it returns 3 or 4 without starting anything. |
| `init [--force]` | Write a starter configuration file. |
| `install-task [--delay HHHH:MM]` | Register a per-user logon task that runs `gamemode-executorw.exe`, with no window. The configuration path is stored absolute. |
| `uninstall-task` | Remove that task. |

Global options: `--config <PATH>`, `--log-level <LEVEL>`, `--version`.

Without `--config`, the file is looked up next to the executable first
(`config.toml`, portable install), then in
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

### Actions

Each event has a mode and a list of commands. `series` runs each command after
the previous one has been waited for; `parallel` starts them all at once and
then waits. A command that fails to start is logged and never prevents the
others: an event is a set of independent side effects, not a pipeline.

| Field | Default | |
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
gone, the watcher starting or stopping, a session recovered at start. Nothing
else competes with those lines.

Each line is `time  LEVEL  category  message`, with the category one of
`watcher`, `game` or `commands`:

```
2026-09-10 17:51:02.433  INFO  watcher   GameModeExecutor 0.1.0 starting
2026-09-10 17:53:14.080  INFO  game      Game detected: bf6.exe
2026-09-10 17:58:41.833  INFO  game      Game no longer detected: bf6.exe
```

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

| | Where | |
| --- | --- | --- |
| Configuration | next to the executable, or `%APPDATA%\GameModeExecutor\config.toml` | yours; roams with the profile |
| Log | `%LOCALAPPDATA%\GameModeExecutor\logs\` | disposable |
| Session marker | `%LOCALAPPDATA%\GameModeExecutor\pending-stop-actions` | present while a game session is open; left behind by a logoff, shutdown or crash, and honoured at the next start. `status` reports it. |
| Logon task | `\GameModeExecutor\Watcher` in Task Scheduler | records the absolute path of the executable |

## Building and releasing

Requires the Rust toolchain, stable, edition 2024. The Windows SDK's `rc.exe`
embeds the icon; without it the build warns and continues.

```powershell
.\scripts\build.ps1            # test
.\scripts\build.ps1 build      # test, then a release build
.\scripts\build.ps1 release    # test, build, and the portable zip in dist\
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

`release` refuses a dirty tree, checks the commit stamped into the binaries is
the commit being built, stages the bundle, zips it into `dist\`, and refuses to
finish if the archive names the building account or if a task template has
lost the placeholders that make it portable.

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
