# Getting started

You want one thing to happen when you start a game, and another when you stop.
That is all this program does. Here is how to get there in about five minutes.

## The two files

It ships as the same program twice. They differ in one way: one has a black
console window, the other has none.

| | |
| --- | --- |
| **`gamemode-executorw.exe`** | The one that works. It starts by itself when you log on and never shows anything — no window, no icon. You never launch it yourself. |
| **`gamemode-executor.exe`** | The one you talk to. Open it in a terminal to set things up, test, or check. It answers, then it is done. It does not keep watching. |

The `w` just means *windowless*, the same convention as `python.exe` and
`pythonw.exe`.

## Where to put the folder

There is nothing to install: unzip it and keep it somewhere. Two things make
the choice worth a moment's thought.

**It has to stay put.** The logon task records the full path to the executable,
so moving the folder afterwards means running `install-task` again.

**It needs to be a folder you can write to**, because your `config.toml` sits
next to the executable.

The tidiest place, and the Windows convention for a program installed for one
user, is:

```
%LOCALAPPDATA%\Programs\GameModeExecutor
```

Anywhere of your own works too — `C:\Tools\GameModeExecutor`, say. Two to
avoid:

| Not here | Why |
| --- | --- |
| `C:\Program Files` | Needs administrator rights to write, and then your own configuration sits in a folder you cannot edit. This program is built to never ask for those rights. |
| A OneDrive or Dropbox folder | Synced folders move files, lock them mid-sync, and can turn them into online-only placeholders. For something that starts at logon, that is a bad bet. |

## Three steps

### 1. Write down what you want to run

```bash
gamemode-executor init
```

That writes a starter `config.toml` into `%APPDATA%\GameModeExecutor` and tells
you where. Open it in any text editor. The part that matters looks like this:

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

One block per command. Add as many as you like to either event.

Write paths between **single quotes** — that way Windows backslashes need no
doubling:

```toml
program = 'C:\Program Files\Something\tool.exe'
```

### 2. Check it

```bash
gamemode-executor validate
```

Either it says the file is fine, or it points at the line and column of the
mistake.

Then try your commands for real, without waiting for a game:

```bash
gamemode-executor trigger start
gamemode-executor trigger stop
```

### 3. Turn it on

```bash
gamemode-executor install-task
```

This registers a task that starts the watcher every time you log on. No
administrator rights, no password, no window.

**That is the end of the setup.** Play. The commands fire by themselves.

Want a complete worked example rather than a blank page? [Recipes](recipes/) has
one per job, each with a `config.toml` you can copy straight over.

## Checking that it is alive

```bash
gamemode-executor status
```

Tells you what it sees right now: whether a game is running, which one, and
whether Windows recognises it.

The log keeps the history of every session. It lives in
`%LOCALAPPDATA%\GameModeExecutor\logs\gamemode-executor.log` and reads like this:

```
2026-09-10 17:51:02.433  INFO  watcher   GameModeExecutor 0.1.0 starting
2026-09-10 17:53:14.080  INFO  game      Game detected: bf6.exe
2026-09-10 17:58:41.833  INFO  game      Game no longer detected: bf6.exe
```

## Using the game's name in your command

If you want the command to know which game started, these get substituted:

| Placeholder | Becomes |
| --- | --- |
| `{event}` | `game_start` or `game_stop` |
| `{process_name}` | e.g. `bf6.exe` |
| `{process_id}` | e.g. `14552` |
| `{process_path}` | the full path, when readable |

```toml
args = ["-Command", "Write-Host 'playing {process_name}'"]
```

Detection never depends on these. A game Windows tracks but does not describe
leaves them empty, and everything still runs.

## If your program needs administrator rights

Some do, usually because they talk to hardware. The watcher runs without
administrator rights on purpose and cannot start those directly: Windows refuses
with error 740.

The way round it is a scheduled task per command, registered once with *run with
highest privileges*. Triggering one needs no rights and raises no prompt, so
your command becomes `schtasks /Run` instead.

[Fan profiles with FanControl](recipes/fancontrol-fan-profiles/) walks through
that case end to end, with ready-made task files.

## When something is not right

**Nothing happens when I start a game.**
Run `gamemode-executor status` while the game is running. If it says no game is
running, Windows itself is not flagging that title — see
[How it works](how-it-works.md).

**It says another instance is already running.**
The background watcher is doing its job. That message means you tried to start a
second one, which is refused so your commands cannot fire twice.

**My command does not run.**
Check the log. A command that fails to start is recorded with the reason, and it
never prevents the others from running. The usual cause is a wrong path, or a
program that needs administrator rights (see above).

**The fans take ages to calm down after I quit.**
That wait is Windows', not this program's. It releases its own "a game is
running" signal when it decides to — sometimes in seconds, sometimes in minutes,
and not predictably per game. There is nothing to tune. The log shows exactly
where the time went if you set `log_level = "debug"`.

## Turning it off

```bash
gamemode-executor uninstall-task
```

Removes the logon task. Nothing else is left running.
