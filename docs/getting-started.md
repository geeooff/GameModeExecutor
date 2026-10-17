# Getting started

Run one thing when a game starts, another when it stops. Five minutes.

## TL;DR

1. Run `GameModeExecutor-<version>.msi` from the
   [latest release](https://github.com/Geeooff/GameModeExecutor/releases/latest).
   If Windows warns: *More info*, *Run anyway*. A grey controller appears
   beside the clock.
2. Right-click it, **Edit configuration**, add your commands
   ([below](#say-what-to-run)), save. It applies at once.
3. Play. The icon turns green while a game runs.

## Installing

**The installer** (`.msi`) needs no administrator rights. It installs for you
alone in `%LOCALAPPDATA%\Programs\GameModeExecutor`, writes a starter
configuration, registers the logon task and starts the watcher. Windows warns
because it is not signed; the release page says how to check it came from this
repository. A newer release installs over it and keeps your configuration.

**The zip** holds the same files. Unzip it somewhere it will stay —
`%LOCALAPPDATA%\Programs\GameModeExecutor` is the convention — not in
`C:\Program Files` (needs administrator rights) nor a synced folder like
OneDrive. Then, once:

```bash
gamemode-executor init
gamemode-executor install-task
```

Moving the folder later means `install-task --force` again.

There are two executables. `gamemode-executorw.exe` is the watcher: no window,
started at logon, never by you. `gamemode-executor.exe` is the one you type
commands into.

## Say what to run

Right-click the icon, **Edit configuration** (the file is
`%APPDATA%\GameModeExecutor\config.toml`). As installed it runs nothing. Add
one block per command:

```toml
[[on_game_start.actions]]
name = "high performance power plan"
program = "powercfg.exe"
args = ["/setactive", "SCHEME_MIN"]

[[on_game_stop.actions]]
name = "balanced power plan"
program = "powercfg.exe"
args = ["/setactive", "SCHEME_BALANCED"]
```

Write paths between single quotes, so backslashes need no doubling:
`program = 'C:\Program Files\Something\tool.exe'`. The starter file has two
beeps, commented out, to hear a detection before writing anything real.

Save, and the watcher reloads it within a second, even mid-game. Only
`log_dir` and `log_days` wait for the next start.

Test without a game:

```bash
gamemode-executor validate
gamemode-executor trigger start
gamemode-executor trigger stop
```

Commands can use the game's name: see the placeholders in the
[reference](reference.md#actions). For a complete example, the
[recipes](recipes/) each have a `config.toml` to copy.

## The icon

| Icon | Meaning |
| --- | --- |
| grey controller | running, no game |
| green controller | a game is detected; the tooltip names it |
| red, slashed | the configuration has a mistake; nothing runs until it is fixed, and the menu's first line says where |

Not visible? Click the `^` next to the clock and drag it onto the taskbar.

| Menu | |
| --- | --- |
| **Edit configuration** | opens `config.toml` |
| **Open log** | opens today's log |
| **Documentation** | this page, for the build you run |
| **Check for updates** | asks GitHub — the only time the program connects to anything |
| **Quit** | stops the watcher, running the stop commands first |

After *Quit*, it starts again at the next logon, or from **GameModeExecutor**
in the Start menu.

## Updating

**Check for updates**, then **Download and install**. It checks the file
against the release's checksums and installs, even mid-game: the new watcher
takes the game over without running any command. From a terminal:
`gamemode-executor update`.

## Checking on it

`gamemode-executor status` says what it sees now. The log, one file a day in
`%LOCALAPPDATA%\GameModeExecutor\logs` (**Open log**), says what happened:

```
2026-09-10 17:51:02.433  INFO  watcher   GameModeExecutor 0.1.0 starting
2026-09-10 17:53:14.080  INFO  game      Game detected: bf6.exe
2026-09-10 17:58:41.833  INFO  game      Game no longer detected: bf6.exe
```

`gamemode-executor --version` names the exact build, with a link to this page
as it was for that build.

## If your program needs administrator rights

The watcher never runs elevated, so Windows refuses such a program (error
740). Register it once as a scheduled task *with highest privileges* and run
`schtasks /Run` instead. [Fan profiles with FanControl](recipes/fancontrol-fan-profiles/)
does exactly that, step by step.

## When something is not right

**Nothing happens when I start a game.** Bring the game to the front, then
run `gamemode-executor status`. If it sees no game, Windows does not count
this title as one: open the Game Bar over it (`Win+G`) and tick *Remember
this is a game*. See [How it works](how-it-works.md#how-it-knows-a-game-is-running).

**A program that is not a game starts my commands.** It was ticked *Remember
this is a game*; the log and `status` say so. Untick it in the Game Bar.

**The log says a game I marked is now known to Microsoft.** Untick the box:
Windows recognises it by itself now.

**My command does not run.** The log says why — usually a wrong path, or a
program that needs administrator rights.

**The icon is red.** The notification and the menu say which line is wrong;
`gamemode-executor validate` gives the full account. Save a fixed file and
the icon turns grey.

**I logged off mid-game and the fans stayed loud.** They calm down at the
next logon: Windows refuses to start programs once the session is ending
([why](how-it-works.md#logging-off-mid-game)).

**The log says the last watcher ended without stopping.** Something ended it
mid-game — Task Manager, a crash, a power cut. That start has run the stop
commands, or resumed the game. If nothing you know of did it,
[open an issue](https://github.com/geeooff/GameModeExecutor/issues) with that
day's log.

**The update failed.** The log's `update` lines give the reason; the old
version keeps running.

**The fans take ages to calm down after I quit.** Windows holds its "a game is
running" signal until you touch the PC again, then lets go within a second to
a minute and a half. *(Corrected 2026-09-25.)*

**It says another instance is already running.** The watcher is already
running; a second one is refused.

## Turning it off

```bash
gamemode-executor stop
gamemode-executor uninstall-task
```

To remove it entirely: *Programs and Features*, or `purge` — see
[Removing it](how-it-works.md#removing-it).

The [reference](reference.md) lists every command, setting and exit code;
[How it works](how-it-works.md) explains the mechanism.
