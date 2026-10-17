# Reference

The exact behaviour. To get started, read [Getting started](getting-started.md);
for the reasons, the [design record](design/).

## The executables

| Executable | For |
| --- | --- |
| `gamemode-executor.exe` | Every command you type. A console program: a shell waits for it and gets its exit code. |
| `gamemode-executorw.exe` | The same commands, no console, prints nothing. Run by the logon task (`run`) and the installer (`stop`, `init`, `install-task`, `uninstall-task`). |
| `presence-probe.exe` | Diagnostics, not shipped. See [Detection](design/00-detection.md#the-instrument). |

## Commands

| Command | What it does |
| --- | --- |
| `run` | Watch and react, in this console. The default. |
| `status` | The build, the presence writer and whether it runs, the session marker, the Known Game List matches ranked by GPU share, and the games marked by hand. |
| `check <path>` | Whether Windows knows that executable as a game; `check --pid <id>` reads a running process instead. |
| `trigger start\|stop` | Run one event's commands now, to test them. |
| `validate` | Check the configuration; exits 3 or 4 on a problem. |
| `init [--force]` | Write the starter configuration to `%APPDATA%\GameModeExecutor`; an existing one is kept unless `--force`. |
| `install-task [--delay 15s] [--force]` | Register the per-user logon task running `gamemode-executorw.exe`, and start it. An existing task is kept unless `--force`. |
| `uninstall-task` | Remove that task. |
| `update [--check]` | Look for a newer release on GitHub; without `--check`, download, verify against `SHA256SUMS.txt` and install it. The only command that connects to anything. |
| `stop [--handover]` | Stop the running watcher as *Quit* does, and wait for it to go. With `--handover`, an open game session is left to the next watcher, nothing run. |
| `purge [--yes]` | Remove every trace: task, configuration, logs, session marker, updates folder, executables. Asks first; refuses during a game. |
| `open <what>` | Hidden: the menu's helper, which opens a file or page the way Explorer would and exits. |

Global options: `--config <PATH>`, `--log-level <LEVEL>`, `--version`.
Without `--config`, `config.toml` next to the executable is used if present,
else `%APPDATA%\GameModeExecutor\config.toml`.

The setup commands (`init`, `install-task`, `uninstall-task`, `stop`) log what
they did under `setup`, `update` under `update`.

## Configuration

TOML; unknown keys are refused with their line and column.
[`config.example.toml`](../config.example.toml) documents every field.

```toml
[general]
stop_actions_on_exit = true   # run the stop commands when the watcher stops mid-game
log_level = "info"            # error | warn | info | debug | trace
#log_dir = 'C:\somewhere'     # default: %LOCALAPPDATA%\GameModeExecutor\logs
#log_days = 7                 # days of log kept, today's included; 1 or more

[detection]
poll_interval = "2s"          # how often to look for a game while idle
stop_delay = "2s"             # grace after the game's signal ends
identify_after = "20s"        # when to ask the GPU which process is the game; again while undecided, six attempts in all
gpu_sample = "1s"

[on_game_start]
mode = "series"               # or "parallel"

[[on_game_start.actions]]
name = "high performance power plan"
program = "powercfg.exe"
args = ["/setactive", "SCHEME_MIN"]
wait = true
timeout = "10s"
```

Write paths between single quotes; backslashes then need no doubling.

The watcher reloads the file within a second of a save, except `log_dir` and
`log_days`, which wait for the next start. A file it cannot use disables it
until a usable one is saved.

### Actions

`series` runs each command after the previous one is waited for; `parallel`
starts them all, then waits. A command that fails never stops the others.

| Field | Default | Meaning |
| --- | --- | --- |
| `name` | the program | label in the log |
| `program` | required | path or name of the executable |
| `args` | `[]` | one string per argument |
| `working_dir` | inherited | |
| `env` | `{}` | extra environment variables |
| `no_window` | `true` | no console window |
| `wait` | `false` | wait for it to exit and record its status |
| `timeout` | none | stop waiting after this; the process keeps running |
| `enabled` | `true` | |

Placeholders, substituted in `program`, `args`, `working_dir` and `env`, empty
when Windows does not name the game:

| Placeholder | Value |
| --- | --- |
| `{event}` | `game_start` or `game_stop` |
| `{process_name}` | e.g. `bf6.exe` |
| `{process_id}` | e.g. `12345`; at `game_stop`, the id from the start, usually gone |
| `{process_path}` | the full path, when readable |

A stop command that fails — a waited one that exits non-zero or times out, or
one that cannot start — runs again once at the next start. At a logoff or shutdown no command
is tried; they run at the next start. A fire-and-forget command started as a
shutdown begins is lost, since nothing reports its failure.

### Programs that require administrator rights

The watcher never runs elevated, so Windows refuses such a program with error
740. Register a scheduled task *with highest privileges* once, and run
`schtasks /Run /TN <task>`; no prompt. The
[FanControl recipe](recipes/fancontrol-fan-profiles/) has templates and a
script. An elevated watcher would let the configuration file run anything as
administrator.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | success |
| 1 | anything else |
| 2 | command line misuse |
| 3 | configuration file not found |
| 4 | configuration invalid |
| 5 | another instance is already running |

3 and 4 come from the commands that need a usable file; the watcher shows the
fault in its icon instead.

## The log

`%LOCALAPPDATA%\GameModeExecutor\logs` (or `log_dir`), one file a day,
`gamemode-executor.YYYY-MM-DD.log`, local time, written as it happens. The day
changes at midnight UTC — the library's only choice. The last `log_days` files
are kept (7), sometimes one more.

| Level | For | Promise |
| --- | --- | --- |
| `error` | anyone | Something needs you, and it says what. |
| `warn` | technician | A degradation absorbed: a command that failed, a marker not written, a watcher that ended without stopping. |
| `info` | anyone | The story of a session. **The default.** |
| `debug` | technician | Why, with every line above annotated. |
| `trace` | technician | Raw measurements. |

`info` is reserved for what the program is for: a game detected, named or
gone; the watcher starting or stopping, Windows ending the session included; a
session recovered at start; the configuration reloaded; what the setup and
update commands did. A configuration the watcher cannot use is the one
`error`.

Each line is `time  LEVEL  category  message`; the category is `watcher`,
`game`, `commands`, `setup` or `update`. At `debug` the same lines carry their
details as fields:

```
2026-09-10 17:53:14.080  INFO  game      Game detected: bf6.exe  pid=14552 matched_by="exe path"
2026-09-10 17:53:35.458  INFO  game      Game identified more precisely: bf6.exe (74% of the rendering)  pid=14552 rendering_share=73.8
```

A crash is a `FATAL:` line at `error`. `RUST_LOG` overrides `log_level`, with
`tracing`'s filter syntax (`RUST_LOG=game=debug`). Several processes can write
the file at once; lines never interleave, since each is one append.

## Files the watcher keeps

| File | Where | Nature |
| --- | --- | --- |
| Configuration | next to the executable, or `%APPDATA%\GameModeExecutor\config.toml` | yours; roams |
| Log | `%LOCALAPPDATA%\GameModeExecutor\logs\` | disposable |
| Session marker | `%LOCALAPPDATA%\GameModeExecutor\pending-stop-actions` | a game session not yet closed. `state = open` while a watcher holds it, `left` once one stopped and left it to the next start; `open` found at a start means that watcher ended without stopping. |
| Fault marker | `%LOCALAPPDATA%\GameModeExecutor\configuration-fault` | the configuration was unusable when last read, so the next usable read says the fault is over |
| Logon task | `\GameModeExecutor\Watcher` | holds the executable's absolute path; removed on uninstall, kept on upgrade |
| Updates | `%LOCALAPPDATA%\GameModeExecutor\updates\` | a release being installed; emptied at the next start, the installer's log kept if the update failed |

## Building and releasing

Rust stable, edition 2024, and the Windows SDK's `rc.exe` for the icon and
version block (without it the build warns and goes on).

```powershell
.\scripts\build.ps1            # fmt, clippy -D warnings, tests, shipped configs, doc links
.\scripts\build.ps1 build      # + release build, PE subsystem check
.\scripts\build.ps1 release    # + clean tree, stamped commit, dist\ zips, MSI and its ICE validation
```

A release is a tag `vX.Y.Z` on the commit that bumps `Cargo.toml` and dates
the version's `CHANGELOG.md` section. The workflow then runs the same script
and publishes the installer, the zips and their checksums, with that section
as the notes. A version the changelog lacks is refused.

From VS Code, `Ctrl+Shift+B` builds; *Run Task* has the same modes and two for
an installed watcher.

## Limits

- Detection is the Game Bar's: a title Windows does not see as a game triggers
  nothing (and gets no Game Mode either), unless you mark it.
- Nothing guarantees only games start the presence writer; `status` and the log
  show what fired.
- One watcher per session; a second exits with code 5.
- The wait between quitting and Windows' release is Windows'
  ([How it works](how-it-works.md#the-wait-after-you-quit)).
