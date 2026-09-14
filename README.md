# GameModeExecutor

A small Windows watcher, written in Rust, that runs the executables you configure
when a game starts and when it stops.

It knows nothing about any particular tool. It just launches programs with
arguments, which is enough to drive
[FanControl](https://github.com/Rem0o/FanControl.Releases) profiles, an RGB CLI, a
power plan switch, or anything else that has a command line.

```
game starts  ->  [[on_game_start]]  ->  your command
game ends    ->  [[on_game_stop]]   ->  your other command
```

FanControl itself needs one extra step, because it requires administrator
rights — see [Programs that require elevation](#programs-that-require-elevation).

**There is no list of games to maintain.** Detection is Windows' own verdict, and
the watcher does not poll while you play.

## Documentation

| | |
| --- | --- |
| **[Getting started](docs/getting-started.md)** | Running in five minutes. Start here. |
| **[Recipes](docs/recipes/)** | Worked examples, start to finish, one folder each with a ready-made `config.toml` — including quiet fans outside games and a game profile while playing, with FanControl. |
| **[How it works](docs/how-it-works.md)** | For the curious: how it knows a game is running, why the two executables, why the wait after you quit. No programming needed. |

The rest of this file is the reference, and the reasoning and measurements
behind the design.

## How it works

Windows ships a Game Bar *presence writer*: an out-of-proc WinRT server it
activates when it decides a game is present, and releases when the game is gone.
The watcher does not replace it or talk to it. It simply observes whether that
process is alive:

- **idle** — look for the writer every `poll_interval` (2 s by default);
- **playing** — park on the writer's process handle with `WaitForMultipleObjects`
  and do nothing at all until Windows releases it.

So the only cost while a game runs is one thread asleep in the kernel.

Which executable to watch is read from the registry at run time, so a machine
where another tool owns the registration is still followed correctly, and a
rename by Windows servicing does not silently break detection. Matching is on the
full image path, not the file name.

## Why that signal, and not the others

The goal was to reuse what Windows already knows instead of maintaining an
allow-list of executables. Everything below was checked on Windows 11 25H2
(build 26200.9445).

| Candidate | Verdict |
| --- | --- |
| **Game Mode** (`expandedresources.h`, `HasExpandedResources`) | Unusable. [Deprecated since Windows 10 1809](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/gamemode/game-mode-portal), and only ever callable from inside the game process. No supported way to ask whether Game Mode is active. |
| **Xbox Mode** (the Xbox full screen experience, internally "Gaming Posture") | Unusable. A shell home-app mode, not a detector, and its APIs are `Windows.Internal.*`. |
| **`Windows.Gaming.Preview.GamesEnumeration.GameList`** | Unusable. Requires the restricted `gameList` capability: *"Unless your developer account is specially provisioned by Microsoft, calls to these APIs will fail at runtime."* |
| **A custom `IPresenceWriter`** | Impossible. The registration key is owned by `NT SERVICE\TrustedInstaller`; `BUILTIN\Administrators` and `NT AUTHORITY\SYSTEM` both hold `ReadKey` only, so an elevated write fails with `0x80070005`. There is no per-user WinRT server registration to fall back on. It would mean taking ownership of a protected system key, which servicing can reset. |
| **Watching the shipped presence writer** | **This.** Free, unprivileged, modifies nothing, and leaves Xbox Live presence alone. |

The one thing all the gaming features share is the Known Game List, synced into
`HKCU\System\GameConfigStore\Children`. That data is used here for naming only —
never for detection.

## The measurements

`presence-probe activate`, unelevated:

| Step | Result |
| --- | --- |
| `RoActivateInstance` on the presence writer class | 40–43 ms, no privileges |
| The writer process appears | 6–7 ms after activation |
| It exits once the last reference is released | under 20 ms, no linger |

So the process tracks the COM reference exactly, on both edges.

Two real sessions, logged with `presence-probe watch`:

| Game | Delivery | Writer up | Writer down | Duration |
| --- | --- | --- | --- | --- |
| Starfield | Game Pass (packaged) | 17:48:29 | 17:52:03 | 3 m 34 s |
| Farming Simulator 25 | Steam (Win32) | 17:59:19 | 18:00:44 | 1 m 24 s |

Both produced **exactly one start/stop pair**. Two alt-tabs to the desktop and
back during the Starfield session did not release the reference, so the writer
does not blink on focus changes — its lifetime is the session.

Starfield was launched into the background and the writer still started, with
another application in the foreground, so detection does not depend on focus.
`GameDVR\LastGameActivity` matched the start timestamp to the second in both
sessions.

As a negative control, launching and focusing Notepad spawned nothing at all.

## Naming the game

Detection knows *that* a game is running, not *which*. The name for the logs and
the action placeholders comes from `HKCU\System\GameConfigStore\Children`, whose
entries fall into two families:

- `Type = 1`, Win32 titles, identified by `MatchedExeFullPath` or
  `ExeParentDirectory`. That second field is inconsistent — sometimes a full
  path, sometimes a bare folder name, and one real entry is just `x64`, so bare
  names that are too generic are ignored rather than trusted.
- `Type = 2`, packaged Store and Game Pass titles, which have no executable path
  at all and are identified by `UtmItemId`, shaped `P~<PackageFamilyName>!<AppId>`.
  Starfield is one of these, so path matching alone would never have named it;
  those are matched by asking the running process for its package family name.

When nothing matches, the actions still run, with the placeholders empty.
Detection never depends on naming.

### Picking the right one

A title is not one process. A launcher stub, an anti-cheat service and the game
share an install folder or a package family, so they all match, and the
satellites usually start first. Naming the session after the first match gave
`gamelaunchhelper.exe` for Starfield and `EAAntiCheat.GameServiceLauncher.exe`
for Battlefield 6.

So a little way into a session — `detection.identify_after`, twenty seconds by
default — the per-process GPU counters are read once, and the candidate that is
actually rendering wins. Only the engines that mean *drawing* count: video
decode and copy engines are busy for a video player too.

This ranks candidates the known game list already produced; it never promotes a
process the list did not match. Reading those counters can be refused depending
on the account, and a game may still be on its loading screen, so no answer is
an expected outcome and simply leaves the first match in place.

`status` shows the ranking, which is the only way to see what would be chosen:

```
matching processes   : 2
    0.2% rendering  gamelaunchhelper.exe (pid 21952, via package family)
   94.7% rendering  forzahorizon6.exe (pid 32728, via package family)
would be named       : forzahorizon6.exe (pid 32728, via package family)
```

The industry does no better. GeForce Experience and Adrenalin match curated
databases of known titles and scan folders; Discord matches a table of
executable names. Only Intel's PresentMon measures the truth, by tracing frame
presentation through ETW, and that needs administrator rights.

## Where this is going

[PLAN.md](PLAN.md) tracks the work in identified lots, what is committed versus
merely considered, and the assumptions still waiting to be verified.

## Install

Requires the Rust toolchain (stable, edition 2024).

```bash
cargo build --release
```

Or go through the checklist that is actually used, which does rather more than
`cargo build` — see [Building and releasing](#building-and-releasing).

Self-contained executables, no runtime dependencies:

| Executable | About | What it is for |
| --- | --- | --- |
| `gamemode-executor.exe` | 1.1 MB | Everything you type. A console program, so a shell waits for it, pipes work and exit codes come back. |
| `gamemode-executorw.exe` | 1.1 MB | Watching, and nothing else. No console at all — this is what the logon task runs. |
| `presence-probe.exe` | 180 KB | Diagnostics. See the end of this file. |

The `w` suffix is the same convention as `python.exe` and `pythonw.exe`, and it
exists for the same reason: a program cannot be both a console and a windowless
one in a single file. The alternative — one windowless binary that attaches to
the terminal it was launched from — was rejected because a shell does not wait
for a windowless process, so `validate`'s exit code would silently stop reaching
scripts.

Both binaries share one library, so the watcher they run is the same code.

## Quick start

```bash
gamemode-executor init        # write a starter config in %APPDATA%\GameModeExecutor
gamemode-executor validate    # check it
gamemode-executor status      # what the detector sees right now
gamemode-executor run         # watch, in the foreground, logging to the console
gamemode-executor install-task  # start it at every logon, no window, no admin needed
```

## Commands

These all belong to `gamemode-executor.exe`. `gamemode-executorw.exe` takes only
`--config` and `--log-level`, and watches.

| Command | What it does |
| --- | --- |
| `run` | Watch and react, in this console. This is the default command. For an unattended instance use `gamemode-executorw.exe`. |
| `status` | Print the registration, whether a game is running, and what the known game list holds. |
| `check <path>` | Ask whether Windows knows a given executable as a game. |
| `trigger start\|stop` | Run one set of actions immediately, ignoring detection. Handy to test your commands. |
| `validate` | Parse and check the configuration. |
| `init [--force]` | Write a starter configuration file. |
| `install-task [--delay HHHH:MM]` | Register a per-user logon task that runs `gamemode-executorw.exe`, with no window. The configuration path is stored absolute. |
| `uninstall-task` | Remove that task. |

Global options: `--config <PATH>`, `--log-level <LEVEL>`.

Without `--config`, the file is looked up next to the executable first
(`config.toml`, portable install), then in
`%APPDATA%\GameModeExecutor\config.toml`.

## Actions

Each event has a mode and a list of commands:

```toml
[on_game_start]
mode = "series"        # or "parallel"

[[on_game_start.actions]]
name = "FanControl - Game profile"
program = "schtasks.exe"
args = ["/Run", "/TN", 'GameModeExecutor\FanControl Game']
wait = true
timeout = "15s"
```

`series` runs each command after the previous one has been waited for.
`parallel` starts them all at once and then waits. Two commands sleeping two
seconds each take 4.5 s in series and 2.2 s in parallel.

A command that fails to start is logged and never prevents the others from
running: an event is a set of independent side effects, not a pipeline.

These placeholders are substituted in `program`, `args`, `working_dir` and
`env`:

| Placeholder | Value |
| --- | --- |
| `{event}` | `game_start` or `game_stop` |
| `{process_name}` | e.g. `FarmingSimulator2025Game.exe` |
| `{process_id}` | e.g. `12345` |
| `{process_path}` | full image path, when readable |

See [`config.example.toml`](config.example.toml) for the annotated reference.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | success |
| 1 | anything else |
| 2 | command line misuse (returned by the argument parser) |
| 3 | configuration file not found |
| 4 | configuration invalid: syntax or validation |
| 5 | another instance is already running |

`validate` is the command to script against: it returns 3 or 4 without starting
anything.

## Reading the log

The log lives in `%LOCALAPPDATA%\GameModeExecutor\logs\gamemode-executor.log` unless
`log_dir` says otherwise, with local timestamps. Run the watcher in a terminal
instead of hidden and the same lines appear there, coloured.

One log serves two readers, and `log_level` is the dial between them.

| Level | Written for | What it promises |
| --- | --- | --- |
| `error` | anyone | Something needs you. It names the file or command and what to check. |
| `warn` | technician | A degradation the program absorbed and carried on from. |
| `info` | anyone | The story of a session, in plain sentences. **The default.** |
| `debug` | technician | Why the program did what it did — and every line above, annotated. |
| `trace` | technician | Raw measurements. |

`info` is reserved for what this program is for: a game detected, named, or
gone, and the watcher starting or stopping. Nothing else is allowed to compete
with those lines.

Each line is `time  LEVEL  category  message`, where the category is one of
`watcher`, `game` or `commands`.

```
2026-09-10 17:51:02.433  INFO  watcher   GameModeExecutor 0.1.0 starting
2026-09-10 17:53:14.080  INFO  game      Game detected: bf6.exe
2026-09-10 17:58:41.833  INFO  game      Game no longer detected: bf6.exe
```

Switching to `debug` does not give you a different log. It gives you the same
one, annotated — the technical detail rides along with each line as fields
rather than in lines of its own:

```
2026-09-10 17:53:14.080  INFO  game      Game detected: bf6.exe  pid=14552 matched_by="exe path"
2026-09-10 17:53:35.458  INFO  game      Game identified more precisely: bf6.exe (74% of the rendering)  pid=14552 rendering_share=73.8
2026-09-10 17:53:35.462 DEBUG  commands  Running the configured commands  count=2 mode="series"
```

`RUST_LOG` overrides `log_level` when set, and takes the usual
`tracing` filter syntax — `RUST_LOG=game=debug` for the detection lines alone.

## Programs that require elevation

The watcher runs unelevated, on purpose. Some programs cannot be started that
way. **FanControl is one of them**: its manifest declares
`requestedExecutionLevel level="requireAdministrator"` because it talks to
hardware, so `CreateProcess` from an unelevated parent fails with error 740,
`ERROR_ELEVATION_REQUIRED`. That is true whatever its command line, so
`FanControl.exe -c Game.json` cannot be an action directly.

The bridge is a **scheduled task per command**, registered once with *run with
highest privileges*. Triggering a task needs no elevation and raises no UAC
prompt, so the action becomes:

```toml
[[on_game_start]]
program = "schtasks.exe"
args = ["/Run", "/TN", 'GameModeExecutor\FanControl Game']
```

Register the task once, from an elevated PowerShell:

```powershell
$exe = 'C:\Path\To\FanControl\FanControl.exe'
$action = New-ScheduledTaskAction -Execute $exe -Argument '-c Game.json'
$principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Highest
Register-ScheduledTask -TaskName 'FanControl Game' -TaskPath '\GameModeExecutor\' -Action $action -Principal $principal -Force
```

Everything this program installs lives in a **`GameModeExecutor` folder** in
Task Scheduler rather than loose at its root — the watcher's own task included.

Registering it with no trigger means it only ever runs when something asks it to.

Ready-made task definitions, and a note on why each setting is what it is, live
with the recipe that uses them:
[Fan profiles with FanControl](docs/recipes/fancontrol-fan-profiles/). Several
of those settings are not obvious and get this wrong in ways that fail silently.

**Why not simply run the watcher elevated?** Because the configuration file lives
in `%APPDATA%` and names arbitrary programs to execute. An elevated watcher would
turn that file into a way to run code as administrator with no prompt — a local
privilege escalation for anything running as the user. With the task bridge the
configuration only names a task; the command itself lives where a
non-administrator cannot change it.

## Why a console app and not a Windows service

The watcher runs as a normal user-session program, started at logon by a
scheduled task:

- **Session 0 isolation.** A service cannot see the interactive desktop, and
  starting a GUI program such as FanControl in the user's session would need
  `CreateProcessAsUser` gymnastics.
- **The target apps are per-user.** FanControl runs in your session, with your
  profile and your settings.
- **No admin rights** anywhere: not for the watcher, not for the logon task, not
  for reading the registration.
- **Easier to debug.** Run it in a console, watch the log, hit Ctrl-C.

## Building and releasing

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

`release` stages the bundle, zips it into `dist\`, and refuses to finish if the
archive names this machine's account or if a task template has lost the
placeholders that make it portable.

This script exists because the checklist was being run by hand, and by hand it
was skipped twice — once committing a failing test, once shipping a scheduled
task with a relative path in it.

**From VS Code:** `Ctrl+Shift+B` builds, and *Terminal → Run Task* offers the
same three plus two for driving the installed watcher — restart it, or follow
its log while you play. They all call this script, so what runs in the editor
is what runs in a terminal.

## presence-probe

The measuring instrument that settled the design. Nothing in `watch` or
`activate` modifies the system or needs admin.

```bash
presence-probe status      # show the registration and whether the writer runs
presence-probe activate    # time the on-demand activation
presence-probe watch 900   # log the writer coming and going
```

`watch` polls every 100 ms so a brief launch is not missed; that is deliberate
for a measurement tool and is not how the watcher works. `install` / `uninstall`
are kept for the record and fail with access denied, for the reason in the table
above.

## Notes and limits

- Detection inherits Game Bar's coverage exactly: a title Windows has not
  recognised as a game will not trigger anything — but it would not have got
  Game Mode either.
- The writer is activated for games, not for ordinary applications, but nothing
  guarantees a game is the only possible cause. `status` and the logs make it
  easy to see what fired.
- `stop_actions_on_exit` runs the stop actions when the watcher shuts down
  gracefully (Ctrl-C). A task killed outright at logoff does not get that chance.
- Only one instance runs per session; a second one exits immediately.

## License

MIT. See [LICENSE](LICENSE).
