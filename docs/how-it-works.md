# How it works

For the curious. To get it running, [Getting started](getting-started.md) is
enough.

## How it knows a game is running

**There is no list of games in this program.** It asks Windows.

When Windows decides a game is running, it starts `GameBarPresenceWriter.exe`,
a Game Bar process that tells Xbox what you play, and stops it when the game is
gone. That process existing *is* Windows' verdict. Windows may wait for the
game to come to the front first.

- **No game:** the watcher looks for that process every two seconds. That is
  its only polling, and it is cheap: process numbers only, names for new ones.
- **Game running:** it stops looking and asks Windows to wake it when the
  process ends. Nothing runs while you play.

Any game Windows knows triggers it, including ones released after this
program.

**Games you marked yourself.** Tick *Remember this is a game* in the Game Bar
(`Win+G`) over a title Windows does not recognise. Windows starts no presence
process for it, so the watcher also reads that list: when a marked program
runs, that is a session, and the watcher waits on the program itself. The log
says *marked as a game by hand*, which is how you find a box ticked by
mistake. At each start the log also names the marked games Microsoft's own
list now knows: untick those, Windows recognises them by itself.

## Naming the game

The name only serves the log and the `{process_name}` placeholder; detection
never depends on it. It comes from the games Windows has seen
(`HKCU\System\GameConfigStore`), matched against running processes.

One game often brings a launcher and an anti-cheat that match too, and start
first. So twenty seconds in, the watcher asks the graphics card which match is
drawing the picture:

```
17:53:14  Game detected: EAAntiCheat.GameServiceLauncher.exe
17:53:35  Game identified more precisely: bf6.exe (74% of the rendering)
```

Nothing drawing yet? It asks again every twenty seconds, two minutes at most.

## The wait after you quit

The stop commands can fire well after you quit. That wait is Windows': it keeps
its signal while nobody touches the PC — two hours, once — and at the keyboard
lets go within a second to a minute and a half. The watcher adds two seconds,
in case the signal blinks. `log_level = "debug"` shows where the time went.
*(Corrected 2026-09-25: this page called the wait unpredictable, then about
twenty seconds.)*

## What it costs your machine

Measured on a gaming PC in September 2026, private memory (Task Manager's
*Commit size*):

| When | Processor | Memory |
| --- | --- | --- |
| Waiting for a game | 0.03 % of one core | about 2 MB |
| During a game | nothing measurable | unchanged |
| Naming a game from the graphics card | | 0.25 MB, once |
| The menu, first opened | | about 1 MB, once |
| *Check for updates* | | about 1 MB, until the next start |

One evening of six hours, five of them in GTA Online: 1.25 seconds of
processor in all.

## Why two executables

A Windows program is built either as a console program, which can print, or
as a windowless one, which cannot. One file cannot be both.
`gamemode-executor.exe` answers your commands in a terminal, and the shell
waits for its result. `gamemode-executorw.exe` runs the watcher from the logon
task and the installer's steps, with no window to flash; it writes to the log
instead. Python does the same with `python.exe` and `pythonw.exe`.

A named lock allows one watcher at a time: a second one is refused, so the
Start menu's **GameModeExecutor** is safe to click.

## What starts it

A per-user scheduled task, at logon, fifteen seconds in. Not a service:
services run in a separate session and cannot see the desktop the game is on.
The task is registered from XML, because the usual shortcut would leave a
72-hour execution limit and two battery conditions on it. The installer stops
the watcher before it replaces or removes the files.

The watcher owns one hidden window: the icon hangs off it, and it is how
Windows tells the watcher the session is ending.

## Logging off mid-game

When you log off or shut down, Windows asks every window whether the session
may end. The watcher says yes, writes `Windows is ending the session, so the
watcher stops`, and starts nothing: a program started at that point dies
before it runs. Measured, not guessed. *Until 2026-10-16 the watcher still
started the stop commands then, and they died.*

So the stop commands run at the **next logon**. While a game runs, a file
says so: `pending-stop-actions` in `%LOCALAPPDATA%\GameModeExecutor`. A game
that ends normally removes it once its stop commands have run. Any other end
leaves it, and the next start runs the stop commands:

```
The stop commands of the last session, with Starfield.exe, did not run then, so they run now
```

A stop command that fails leaves the file too, for one more try at the next
start; Windows refuses them that way when a game closes with the shutdown. If
the watcher itself was ended mid-game — Task Manager, a crash, a power cut —
the next start says so at `warn`.

The same file hands a game from one watcher to the next during an update or a
configuration reload: nothing runs, and the new watcher takes the session up
where it was. `stop_actions_on_exit = false` skips the stop commands on *Quit*
and at logoff alike.

## Changing the configuration

The watcher waits on Windows' change notification for the configuration's
folder. When the file changes, it stops the engine and starts one on the new
file, a quarter of a second later; the log says `Configuration reloaded`. A
game in progress carries on, as for an update.

A file that cannot be used **disables the watcher outright**: red icon, a
notification, the mistake on the menu's first line, nothing watched until a
usable file is saved. It never falls back to the last good file, which would
run something other than what the file says. `log_dir` and `log_days` wait for
the next start.

## Updating itself

*Check for updates* asks GitHub where `releases/latest` points — one request,
only when you click. *Download and install* fetches the installer (or the zip,
for a hand-installed copy), checks its SHA-256 against the release's
`SHA256SUMS.txt` and deletes it on a mismatch. The installer then stops the
watcher with a handover, replaces the files and starts the new version. A
zip copy is updated by a hidden shell that waits for the watcher to exit,
unpacks the zip over the folder, keeping the old executables as `.old`, and
runs `install-task`.

The hash proves the file is the one the release published; the program has no
code signature ([why](design/13-updating.md)).

## Removing it

*Programs and Features* removes the executables and the logon task, and keeps
your configuration and log, as Windows applications do.

```
gamemode-executor purge
```

removes every trace after a `yes`: the task, the configuration, every log
file, the session marker, the updates folder, then the executables. It refuses
during a game, and leaves what it did not create, such as a recipe's tasks.

## What it does not do

- **No network unasked.** One request to GitHub, on *Check for updates*. No
  telemetry.
- **No administrator rights.** Programs that need them go through a scheduled
  task.
- **Nothing in your games.** It reads which processes exist and the GPU
  counters; it injects nothing and stays clear of anti-cheat.
- **Nothing changed in Windows.** The Game Bar registration is only read.

## Where things are

| What | Where |
| --- | --- |
| The program | `%LOCALAPPDATA%\Programs\GameModeExecutor`, or wherever you unzipped it |
| Configuration | `config.toml` next to the executable if there is one, else `%APPDATA%\GameModeExecutor\config.toml` (roams with your profile) |
| Log | `%LOCALAPPDATA%\GameModeExecutor\logs`, one file a day, seven kept; the day changes at midnight UTC |
| Scheduled tasks | the `GameModeExecutor` folder in Task Scheduler |

The [design record](design/) has the measurements behind all this, starting
with [Detection](design/00-detection.md); the [reference](reference.md) has
every command and setting.
