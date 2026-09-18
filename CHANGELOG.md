# Changelog

What changed for the person running the program, one section per release,
newest first. Each line is something you can see or do; the commits behind
it are listed on the release page, under *For the curious*, and the
reasoning is in the [design record](docs/design/README.md). The shape is
[Keep a Changelog](https://keepachangelog.com/): *Added*, *Changed*,
*Fixed*, *Removed*.

The release workflow takes the section for the tag it is given and refuses
to publish without one. `AGENTS.md` says how a section is written.

## [Unreleased]

### Added

- **Check for updates**, in the icon's menu: asks GitHub whether a newer
  release exists — the only time the program ever connects to anything,
  and only when you click. The answer comes as a silent notification and
  waits in the menu: *Up to date*, or **Download and install** beside
  **What changed**, which opens the release page.
- **Download and install** fetches the new release, checks it against the
  checksums the release publishes, and installs it — the installer for an
  installed copy, the zip for one unpacked by hand. The icon disappears for
  a second and comes back on the new version, which says so with a
  notification at its first start.
- Updating in the middle of a game keeps your commands out of it: the old
  watcher hands the game session to the new one, which picks it up where it
  was. Nothing runs twice, and your gaming configuration is never switched
  off and on again mid-game.
- `gamemode-executor update` does the same from a terminal; `update --check`
  only asks.
- `gamemode-executor stop --handover` stops the watcher and leaves an open
  game session to the next one, for anyone restarting it by hand during a
  game.
- The log gains an `update` category: every request, every verdict and
  every step of an install, with the reason when something fails.

### Changed

- The installer and the zip now carry the same four files: the two
  executables, `LICENSE.txt` and `README.txt`. The zip no longer ships a
  copy of the documentation; the readme links the pages and the recipes for
  the exact build you have.
- Upgrading with the installer no longer closes a game session: the new
  watcher resumes it.
- The recipes say to restart the watcher after copying a configuration —
  `stop`, then `install-task` — which an installed copy needs to read the
  new file.
- The release page says where the program was measured, Windows 11, and
  what Windows 10 shares with it, rather than claiming both.

## [0.1.0] - 2026-09-18

The first release. Runs the executables you configure when a game starts
and when it stops, on Windows' own signal that a game is running.

### Added

- Detection through the Game Bar presence writer Windows starts for a game
  and stops when it is gone: no list of games to maintain, nothing polled
  while you play.
- A configuration file, `config.toml`, naming the commands to run on each
  edge, in series or in parallel, with placeholders for the game's name and
  path; `validate` to check it, `trigger start|stop` to try the commands
  without a game.
- The game's name in the log and in the placeholders, refined a little way
  into the session by asking the GPU which process is really drawing.
- A notification area icon — grey without a game, green with one — with a
  menu: edit the configuration, open the log, open the documentation for
  the exact build running, quit.
- A windowless twin, `gamemode-executorw.exe`, started at logon by a task
  `install-task` registers; the console executable is the one you type
  commands into.
- Recovery after a logoff, shutdown or crash mid-game: the stop commands
  run at the next start, so you are not left on a gaming configuration.
- A log for two readers: plain sentences at `info`, the technical detail as
  fields at `debug`.
- A per-user installer, `.msi`, that needs no administrator rights, writes a
  starter configuration if you have none, registers the logon task and
  starts the watcher; a `.zip` for anyone who would rather not run an
  installer. Uninstalling removes the executables and the task and leaves
  your configuration; `purge` removes every trace on request.
- Recipes with ready-made configurations: fan profiles with FanControl,
  including the scheduled tasks a program that needs administrator rights
  requires, and a Windows power plan.

[Unreleased]: https://github.com/Geeooff/GameModeExecutor/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Geeooff/GameModeExecutor/releases/tag/v0.1.0
