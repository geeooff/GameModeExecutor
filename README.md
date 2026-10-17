# GameModeExecutor

A small Windows program, written in Rust, that runs the commands you configure
when a game starts and when it stops — fan curves, a power plan, RGB
lighting, anything with a command line.

**No list of games to maintain.** Windows decides when a game is running: the
watcher follows the Game Bar's own signal and the games you marked in the Game
Bar. It does not poll while you play, needs no administrator rights, connects
to nothing unless you ask for an update, and shows only an icon beside the
clock.

## Install

Run the `.msi` from the
[latest release](https://github.com/Geeooff/GameModeExecutor/releases/latest):
no administrator prompt, and the icon appears beside the clock. Right-click
it, *Edit configuration*, and say what to run. [Getting started](docs/getting-started.md)
covers the zip, updates and removal.

## Documentation

| Page | For |
| --- | --- |
| [Getting started](docs/getting-started.md) | Running in five minutes. Start here. |
| [Recipes](docs/recipes/) | Worked examples with a ready-made `config.toml`, FanControl among them. |
| [How it works](docs/how-it-works.md) | How it knows a game is running, and why it behaves as it does. |
| [Reference](docs/reference.md) | Commands, settings, exit codes, the log, building. |
| [Changelog](CHANGELOG.md) | What each release changed. |
| [Design record](docs/design/) | The decisions and the measurements behind them. |
| [AGENTS.md](AGENTS.md) | How to work in this repository. |

## Status

In daily use, released on [GitHub](https://github.com/Geeooff/GameModeExecutor/releases).
Developed on Windows 11 25H2. It needs the Game Bar component Windows ships by
default.

## License

MIT. See [LICENSE](LICENSE).
