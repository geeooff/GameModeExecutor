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
starts for one, and follows the games you marked yourself in the Game Bar —
and it does not poll while you play. It runs as you, with no
administrator rights, connects to nothing unless you ask it to look for an
update, and shows nothing but a small icon in the notification area.

## Documentation

| Page | What it is for |
| --- | --- |
| [Getting started](docs/getting-started.md) | Running in five minutes. Start here. |
| [Recipes](docs/recipes/) | Worked examples, one folder each with a ready-made `config.toml` — including fan profiles with FanControl, which needs one extra step because it requires administrator rights. |
| [How it works](docs/how-it-works.md) | For the curious: how it knows a game is running, why there are two executables, why the wait after you quit. No programming needed. |
| [Reference](docs/reference.md) | Commands, configuration fields, exit codes, the log contract, building. |
| [Changelog](CHANGELOG.md) | What each release changed for you, in plain sentences. |
| [Design record](docs/design/) | Why it is built this way: the decisions, the measurements behind them, and what is still open. |
| [AGENTS.md](AGENTS.md) | How to work in this repository — for coding agents, and for people. |

## Status

In daily use on its author's machine, and released from
[the releases page](https://github.com/Geeooff/GameModeExecutor/releases).
Developed and measured on Windows 11 25H2. It relies on the Game Bar
component Windows ships by default, so a machine where Game Bar has been
removed will not detect anything.

## Install

Two self-contained executables, no runtime dependencies. `gamemode-executor.exe`
is the one you type commands into; `gamemode-executorw.exe` is the same program
with no console, started at logon by a task it registers for you.

From the [latest release](https://github.com/Geeooff/GameModeExecutor/releases/latest),
take the **`.msi`** and run it: per user, no administrator prompt, into
`%LOCALAPPDATA%\Programs\GameModeExecutor`. It writes a starter
configuration, registers the logon task and starts the watcher — the icon
appearing beside the clock is the confirmation. Right-click it, *Edit
configuration*, and say what to run.

The **`.zip`** holds the same executables for anyone who would rather unpack
them by hand; then:

```powershell
gamemode-executor init          # write the starter config.toml
gamemode-executor install-task  # start the watcher now and at every logon
```

Later releases install themselves: right-click the icon, **Check for
updates**, and the menu offers the newer version — nothing is checked
unless you ask. To remove it, *Programs and Features* takes the executables
and the logon task away and leaves your configuration; `gamemode-executor
purge` removes every trace. Building
from source is in the [reference](docs/reference.md#building-and-releasing).

## License

MIT. See [LICENSE](LICENSE).
