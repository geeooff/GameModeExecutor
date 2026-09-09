# GameModeExecutor

A small Windows watcher, written in Rust, that detects when a game is running and
runs the executables you configure — one set when the game starts, another when it
stops.

It knows nothing about any particular tool. It just launches programs with
arguments, which is enough to drive
[FanControl](https://github.com/Rem0o/FanControl.Releases) profiles, an RGB CLI, a
power plan switch, or anything else that has a command line.

```
game detected  ->  [[on_game_start]]  ->  FanControl.exe -c gaming.json
game gone      ->  [[on_game_stop]]   ->  FanControl.exe -c silent.json
```

## Why a console app and not a Windows service

The watcher runs as a normal user-session program, started at logon by a scheduled
task. That is a deliberate choice:

- **Session 0 isolation.** A service runs in session 0. It cannot see the
  interactive desktop, so `SHQueryUserNotificationState` (the full-screen detector)
  fails there, and starting a GUI program such as FanControl in the user's session
  would need `CreateProcessAsUser` gymnastics.
- **The target apps are per-user.** FanControl runs in your session, with your
  profile and your settings. The thing that drives it belongs there too.
- **No admin rights.** Installing a service requires elevation; a per-user logon
  task does not.
- **Easier to debug.** Run it in a console, watch the log, hit Ctrl-C.

The detection and action code lives in plain modules, so wrapping it in a service
later (with the `windows-service` crate) would not require rewriting anything —
but it is not the right default.

## Install

Requires the Rust toolchain (stable, edition 2024).

```bash
cargo build --release
```

The result is a single self-contained `target/release/gamemode-executor.exe`.
Copy it wherever you like.

## Quick start

```bash
# write a starter config in %APPDATA%\GameModeExecutor\config.toml
gamemode-executor init

# edit it, then check it
gamemode-executor validate

# see what the detectors currently think
gamemode-executor status

# watch, in the foreground, with logs on the console
gamemode-executor run

# start it hidden at every logon (no admin needed)
gamemode-executor install-task
```

## Commands

| Command | What it does |
| --- | --- |
| `run [--hidden]` | Watch and react. `--hidden` hides the console and logs to a file. This is the default command. |
| `status` | Print the current process count, shell notification state, foreground process and detection verdict, then exit. |
| `trigger start\|stop` | Run one set of actions immediately, ignoring detection. Handy to test your commands. |
| `validate` | Parse and check the configuration. |
| `init [--force]` | Write a starter configuration file. |
| `install-task [--delay HHHH:MM]` | Register a per-user logon task that runs the watcher hidden. |
| `uninstall-task` | Remove that task. |

Global options: `--config <PATH>`, `--log-level <LEVEL>`.

Without `--config`, the file is looked up next to the executable first
(`config.toml`, portable install), then in
`%APPDATA%\GameModeExecutor\config.toml`.

## Detection

Two detectors, combined with `detection.match_mode` (`any` or `all`):

- **`processes`** — a list of executable names, with or without the `.exe` suffix,
  matched case-insensitively against the running process list. Precise, and the one
  you want for a known set of games.
- **`fullscreen`** — the Windows shell notification state, which reports whether a
  full-screen application owns the desktop. Catches games you did not list, but
  also fires on full-screen video players, hence `ignore_processes`.

`start_delay` and `stop_delay` debounce both directions, so a loading screen or a
quick alt-tab does not flip your fan profile back and forth.

## Actions

Each `[[on_game_start]]` / `[[on_game_stop]]` entry starts one executable. The
following placeholders are substituted in `program`, `args`, `working_dir` and
`env` values:

| Placeholder | Value |
| --- | --- |
| `{event}` | `game_start` or `game_stop` |
| `{process_name}` | e.g. `cs2.exe` |
| `{process_id}` | e.g. `12345` |
| `{process_path}` | full image path, when readable |

See [`config.example.toml`](config.example.toml) for the full annotated reference.

## Notes and limits

- The full-screen detector needs an interactive session; it reports an error under
  session 0. Another reason not to run this as a service.
- `stop_actions_on_exit` runs the stop actions when the watcher shuts down
  gracefully (Ctrl-C). A task killed outright by the scheduler at logoff does not
  get that chance.
- Only one instance runs per session; a second one exits immediately.

## License

MIT. See [LICENSE](LICENSE).
