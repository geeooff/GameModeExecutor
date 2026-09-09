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

## Where this is going

[PLAN.md](PLAN.md) tracks the work in identified lots, what is committed versus
merely considered, and the assumptions still waiting to be verified.

## Install

Requires the Rust toolchain (stable, edition 2024).

```bash
cargo build --release
```

Two self-contained executables, no runtime dependencies:
`gamemode-executor.exe` (about 1.1 MB) and `presence-probe.exe` (about 180 KB).

## Quick start

```bash
gamemode-executor init        # write a starter config in %APPDATA%\GameModeExecutor
gamemode-executor validate    # check it
gamemode-executor status      # what the detector sees right now
gamemode-executor run         # watch, in the foreground, logging to the console
gamemode-executor install-task  # start it hidden at every logon, no admin needed
```

## Commands

| Command | What it does |
| --- | --- |
| `run [--hidden]` | Watch and react. `--hidden` hides the console and logs to a file. This is the default command. |
| `status` | Print the registration, whether a game is running, and what the known game list holds. |
| `check <path>` | Ask whether Windows knows a given executable as a game. |
| `trigger start\|stop` | Run one set of actions immediately, ignoring detection. Handy to test your commands. |
| `validate` | Parse and check the configuration. |
| `init [--force]` | Write a starter configuration file. |
| `install-task [--delay HHHH:MM]` | Register a per-user logon task that runs the watcher hidden. |
| `uninstall-task` | Remove that task. |

Global options: `--config <PATH>`, `--log-level <LEVEL>`.

Without `--config`, the file is looked up next to the executable first
(`config.toml`, portable install), then in
`%APPDATA%\GameModeExecutor\config.toml`.

## Actions

Each `[[on_game_start]]` / `[[on_game_stop]]` entry starts one executable. These
placeholders are substituted in `program`, `args`, `working_dir` and `env`:

| Placeholder | Value |
| --- | --- |
| `{event}` | `game_start` or `game_stop` |
| `{process_name}` | e.g. `FarmingSimulator2025Game.exe` |
| `{process_id}` | e.g. `12345` |
| `{process_path}` | full image path, when readable |

See [`config.example.toml`](config.example.toml) for the annotated reference.

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
args = ["/Run", "/TN", "GameModeExecutor - FanControl Game"]
```

Register the task once, from an elevated PowerShell:

```powershell
$exe = 'C:\Path\To\FanControl\FanControl.exe'
$action = New-ScheduledTaskAction -Execute $exe -Argument '-c Game.json'
$principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Highest
Register-ScheduledTask -TaskName 'GameModeExecutor - FanControl Game' -Action $action -Principal $principal -Force
```

Registering it with no trigger means it only ever runs when something asks it to.

Ready-made task definitions live in [`tasks/`](tasks/), with a note on why each
setting is what it is — several of them are not obvious and get this wrong in
ways that fail silently.

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
