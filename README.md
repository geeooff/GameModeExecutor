# GameModeExecutor

A small Windows program, written in Rust, that runs the executables you
configure when a game starts and when it stops.

```
game starts  ->  your commands
game ends    ->  your other commands
```

It knows nothing about any particular tool. It launches programs with
arguments, which is enough to switch fan profiles, a power plan, RGB lighting,
or anything else with a command line.

**There is no list of games to maintain.** Windows itself decides when a game
is running — the watcher observes the Game Bar presence writer that Windows
starts for one — and it does not poll while you play. It runs as you, with no
administrator rights, keeps no network connection, and shows nothing but a
small icon in the notification area.

## Documentation

| | |
| --- | --- |
| [Getting started](docs/getting-started.md) | Running in five minutes. Start here. |
| [Recipes](docs/recipes/) | Worked examples, one folder each with a ready-made `config.toml` — including fan profiles with FanControl, which needs one extra step because it requires administrator rights. |
| [How it works](docs/how-it-works.md) | For the curious: how it knows a game is running, why there are two executables, why the wait after you quit. No programming needed. |
| [Reference](docs/reference.md) | Commands, configuration fields, exit codes, the log contract, building. |
| [Design record](docs/design/) | Why it is built this way: the decisions, the measurements behind them, and what is still open. |
| [AGENTS.md](AGENTS.md) | How to work in this repository — for coding agents, and for people. |

## Install

Two self-contained executables, no runtime dependencies. `gamemode-executor.exe`
is the one you type commands into; `gamemode-executorw.exe` is the same watcher
with no console, started at logon by a task it registers for you.

Until a release exists, build from source with the stable Rust toolchain:

```powershell
.\scripts\build.ps1 release
```

That runs the checks, builds, and leaves a portable zip in `dist\`. Unzip it
in `%LOCALAPPDATA%\Programs\GameModeExecutor`, then:

```bash
gamemode-executor init          # write a starter config.toml
gamemode-executor validate      # check it
gamemode-executor install-task  # start the watcher at every logon
```

## License

MIT. See [LICENSE](LICENSE).
