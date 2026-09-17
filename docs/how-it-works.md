# How it works

For the curious. No programming needed — but it assumes you are comfortable with
the idea of processes, files and scheduled tasks on Windows.

If you only want it working, [Getting started](getting-started.md) is enough.

## How it knows a game is running

**There is no list of games in this program.** None. That matters, because every
list is out of date the day it ships, and yours would need editing every time
you install something.

Instead it asks Windows, indirectly but reliably.

Windows ships a component for the Game Bar — the overlay you get with `Win+G`.
When Windows decides a game is running, it starts a small process called
`GameBarPresenceWriter.exe`. When the game is gone, Windows shuts that process
down. That process existing *is* Windows' own verdict that you are playing.

So the watcher does something very simple: it watches whether that process
exists.

- **No game:** it looks every two seconds. That is the only polling it ever does.
- **Game running:** it stops looking entirely, and asks Windows to wake it when
  that process ends. Zero activity while you play — which is rather the point of
  a program that runs during games.

The result is that anything Windows treats as a game triggers it, including
titles released after this program was written.

Which process to watch is read from the registry at startup rather than
hard-coded, so a machine where that registration differs still works.

## Naming the game

Detecting and *naming* are two separate things here, and only the first one
matters.

The name is a convenience — for the log, and for the `{process_name}` you can
put in your commands. It comes from a list Windows itself maintains of titles it
has seen, under `HKCU\System\GameConfigStore`. The watcher matches running
processes against it.

The catch is that a single game brings several processes that all match: a
launcher, an anti-cheat service, and the game. The satellites usually start
first, so the first match is often the wrong one.

So, twenty seconds into a session, the watcher asks the graphics card. Windows
exposes per-process GPU counters — the same numbers Task Manager shows in its
"GPU engine" column — and among the processes that already matched, the one
actually drawing the picture is the game. Video decoding and copy engines are
excluded, otherwise a video player would outrank a paused game.

A real session looked like this:

```
17:53:14  Game detected: EAAntiCheat.GameServiceLauncher.exe
17:53:35  Game identified more precisely: bf6.exe (74% of the rendering)
```

This only ever changes the *name*. If the counters cannot be read, or the game
is still on a loading screen, the watcher keeps whatever matched first and
carries on. Nothing about detection depends on it.

## The wait after you quit

You will notice it, so it is worth explaining: the commands for "game stopped"
can fire well after you have actually quit.

Almost none of that delay belongs to this program. Windows keeps its "a game is
running" signal alive for a while after the game itself is gone, and how long is
genuinely unpredictable. Two titles, each measured twice on the same machine on
the same afternoon:

| Title | First measurement | Second |
| --- | --- | --- |
| Starfield | 52 seconds | 3 seconds |
| Battlefield 6 | 4 seconds | 2 minutes |

Same game, same machine, wildly different. So there is no setting to tune and no
table of games that would predict it. This program's own share is about two
seconds — a deliberate grace period, in case Windows briefly restarts that
signal mid-session, which does happen.

Set `log_level = "debug"` and the log states plainly when Windows released the
signal and whether the game had already exited by then.

## Why there are two executables

Windows makes a program choose, when it is built, between two kinds:

- a **console** program, which owns a black window and can print into it;
- a **windowless** program, which has nowhere to print at all.

It is not a setting you flip afterwards. So a program that must both sit
silently in the background *and* answer you when you type commands cannot be one
file.

Hence the pair. `gamemode-executorw.exe` watches and says nothing;
`gamemode-executor.exe` is everything you type. Python solves the same problem
the same way, with `python.exe` and `pythonw.exe`.

The alternative — one windowless program that borrows the terminal it was
launched from — was tried on paper and rejected: a shell does not wait for a
windowless program, so the prompt comes back before the output, and commands
like `validate` would stop reporting success or failure to any script using
them. Silently, which is the worst way for that to break.

Both files are built from the same code, so the watcher they run is identical.

## Only one watcher at a time

A named lock makes sure of it. If you run `gamemode-executor run` while the
background one is going, the second one refuses and exits with a distinct code
rather than doubling up your commands.

## What starts it, and why not a service

A **per-user scheduled task**, triggered at logon, fifteen seconds in.

A Windows service was considered and dropped. Services run before anyone logs
on, in a separate session, where they cannot see the desktop the game is on —
which is precisely what this needs to observe. A task also needs no
administrator rights to install and no password.

The task is registered from a full XML definition rather than the usual
command-line shortcut, because the shortcut leaves three defaults behind that
are wrong for something meant to run forever: a 72-hour execution limit that
kills it after three days, and two battery settings that stop it on an unplugged
laptop.

## The window you cannot see

The windowless watcher does own one window — created, never shown. It is what
the notification icon hangs off, and it is how the watcher hears the shell:
theme changes, and the moment Windows starts ending your session.

## Logging off mid-game

When you log off or shut down with a game running, Windows asks every window
whether it may end the session. The watcher says yes and starts your "game
stopped" commands at once — and they die. That is measured, not guessed: a
program started even one millisecond after that question fails to initialise,
because the session it would live in is already being torn down, and nothing
the watcher does can come earlier than the question.

So the stop commands run at the **next logon** instead. While a game is
running, a small file in `%LOCALAPPDATA%\GameModeExecutor` —
`pending-stop-actions`, no extension — says so. A game that stops normally
removes it. A session that ends any other way —
logoff, shutdown, a crash, a power cut — leaves it behind, and the watcher's
first act at your next logon is to run the stop commands and remove it. The log
reads:

```
The last session ended with Starfield.exe still running and its stop commands never ran, so they run now
```

The delay is the time between logging off and logging back on, plus a few
seconds for the watcher to start. Across a shutdown, that is time the machine
is off.

`stop_actions_on_exit = false` opts out of both: quitting the watcher mid-game
leaves your profile alone, and so does the next logon.

`gamemode-executor status` shows whether that file is there, and where.

## What it does not do

- **No network.** It never connects to anything, and there is no telemetry.
- **No administrator rights.** It runs as you, deliberately. That is why programs
  needing elevation go through a scheduled task instead.
- **It does not touch your games.** It reads which processes exist and what the
  GPU counters say. It never injects anything, never modifies a game, and never
  interferes with anti-cheat.
- **It does not modify Windows.** The Game Bar registration is read, never
  written.

## Where things are

| What | Where |
| --- | --- |
| The program | wherever you put it. `%LOCALAPPDATA%\Programs\GameModeExecutor` is the Windows convention for a per-user install, and it stays writable, which `C:\Program Files` would not |
| Configuration | `config.toml` next to the executable if there is one, otherwise `%APPDATA%\GameModeExecutor\config.toml` |
| Log | `%LOCALAPPDATA%\GameModeExecutor\logs\gamemode-executor.log`, one file, local timestamps |
| Scheduled tasks | a `GameModeExecutor` folder in Task Scheduler, holding `Watcher` and anything a recipe added |

**Roaming for the configuration, Local for the log**, and the split is
deliberate. Windows carries `%APPDATA%` between machines on a roaming profile
and leaves `%LOCALAPPDATA%` behind. The configuration is worth carrying — it
names no path of its own, because anything needing administrator rights is
reached through a scheduled task, and the task holds the machine-specific part.
A log is the opposite: it describes one machine's sessions, and copying it back
and forth at every logon would achieve nothing.

## Going further

The [design record](design/) has the measurements behind all of the above,
and why every other detection mechanism was ruled out — start with
[Detection](design/00-detection.md). The [reference](reference.md) has every
command, configuration field and exit code.
