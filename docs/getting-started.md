# Getting started

You want one thing to happen when you start a game, and another when you stop.
That is all this program does. Here is how to get there in about five minutes.

## The two files

It ships as the same program twice. They differ in one way: one has a black
console window, the other has none.

| Executable | Role |
| --- | --- |
| **`gamemode-executorw.exe`** | The one that works. It starts by itself when you log on and never shows anything — no window, no output. You never launch it yourself; the logon task and the installer do. |
| **`gamemode-executor.exe`** | The one you talk to. Open it in a terminal to set things up, test, or check. It answers, then it is done. It does not keep watching. |

The `w` just means *windowless*, the same convention as `python.exe` and
`pythonw.exe`.

## Installing

The release page offers two files that hold the same two executables.

**The installer, `GameModeExecutor-<version>.msi`.** Run it. Windows may
stop you first with *Windows protected your PC*, because the installer is
not signed with a certificate it knows: *More info*, then *Run anyway*. The
release page lists the file's SHA-256 if you want to check what you
downloaded. It asks for no administrator rights and installs for you alone,
into
`%LOCALAPPDATA%\Programs\GameModeExecutor`. It also writes a starter
configuration if you have none, registers the logon task, and starts the
watcher: the confirmation that it worked is the **grey controller icon**
that appears in the notification area, beside the clock. No window to click
through, and *Programs and Features* lists it afterwards. A newer release
installs over it the same way: it stops the running watcher, replaces the
files, leaves your configuration and your task where they are, and starts
the watcher again. Skip to [Say what to run](#say-what-to-run).

**The zip, `GameModeExecutor-<version>.zip`,** for anyone who would rather
not run an installer: unzip it and keep it somewhere. Two things make the
choice of somewhere worth a moment's thought.

**It has to stay put.** The logon task records the full path to the executable,
so moving the folder afterwards means running `install-task --force` again.

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

## Say what to run

The starter configuration runs nothing: as installed, the watcher detects
games, names them and logs them, and that is all — which is a fine way to see
it work before deciding what it should do. Right-click the icon and choose
**Edit configuration**, or open `%APPDATA%\GameModeExecutor\config.toml`
yourself. (From the zip, `gamemode-executor init` writes that file first.)

The part that matters looks like this:

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

One block per command. Add as many as you like to either event. The starter
file carries two commands that beep, commented out: uncomment them to *hear*
a game being detected before you write anything real.

Write paths between **single quotes** — that way Windows backslashes need no
doubling:

```toml
program = 'C:\Program Files\Something\tool.exe'
```

## Check it

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

Save the file, and that is all: the watcher notices within a second, reads
it again and writes `Configuration reloaded` in the log. Nothing to restart,
and a game in progress is not disturbed — the commands that run when it ends
are the ones you just saved. The one setting that waits for the next start
is `log_dir`, since the log is already open; the log says so.

If the file cannot be used, the icon turns **red, with a slash**, and the
first line of its menu says what is wrong — the line number and the
parser's words, such as `Configuration error: line 3: unknown field
'log_levl'`. Nothing runs until you fix it: not the old commands, not their
stop half. **Edit configuration** still opens the file, and saving a good
one brings the icon back to grey. The same words are in the log, marked
`ERROR`.

**That is the end of the setup.** Play. The commands fire by themselves.

From the zip, one more command the first time, to register the task that
starts the watcher at every logon and to start it now:

```bash
gamemode-executor install-task
```

No administrator rights, no password, no window. The installer did this for
you.

Want a complete worked example rather than a blank page? [Recipes](recipes/) has
one per job, each with a `config.toml` you can copy straight over.

## The notification area icon

Once it is running you get a small controller in the notification area, beside
the clock. It is the only thing this program ever puts on screen.

| Icon | Meaning |
| --- | --- |
| **grey controller** | running, no game. What you will see almost all the time. |
| **green controller** | a game is detected |
| **red controller, slashed** | the configuration cannot be used and nothing is watched until it is fixed; the menu's first line says what is wrong |

Hover it and the tooltip names the game. Right-click and the first line of the
menu says the same — it is greyed out because it is an answer, not a button.

A game Windows tracks but does not describe shows as *"A game is running, but
Windows does not name it"*. That is not a failure: detection never depended on
the name. See [How it works](how-it-works.md#naming-the-game).

Windows hides new icons by default: if you cannot see it, click the **`^`**
arrow next to the clock, and drag it onto the taskbar to keep it there.

**Right-click it** for a short menu:

| Entry | What it does |
| --- | --- |
| **Edit configuration** | opens your `config.toml` in whatever you use for text files — Notepad if `.toml` is not associated with anything |
| **Open log** | opens the log the same way |
| **Documentation** | opens this page for **the exact build you are running**, not for whatever the project looks like today |
| **Check for updates** | asks GitHub whether a newer release exists — the only time this program ever connects to anything, and only when you click. The menu closes, as menus do; the answer arrives as a silent notification a second later, and waits in the menu too: *0.1.0 is the latest version*, or **Download and install 0.2.0** beside a **What changed in 0.2.0** that opens the release page |
| **Quit** | stops the watcher, running the stop commands on the way out so you are not left on a gaming profile |

Quitting only stops it until the next time you log on. To stop it for good, see
[Turning it off](#turning-it-off).

## Updating

Right-click the icon, **Check for updates**. The menu closes and a
notification answers a second later; the answer waits in the menu too. If a
newer release exists, **Download and install** fetches it, checks it against
the checksums the release publishes, and installs it — in the middle of a
game if you like: the running watcher hands the game over to the new one,
which picks it up where it was without touching your commands. The icon
disappears for about a second and comes back, and the new version says so
with a notification, since the install itself is too quick to watch. The
menu and the log, under `update`, say the same.

The same from a terminal:

```bash
gamemode-executor update --check
gamemode-executor update
```

Nothing is ever checked or downloaded unless you ask. If a check or an
install fails, the menu says so in one line ending in *(see log)*, and the
log has the reason — no connection, a refusal from GitHub, a file that did
not verify, or Windows Installer's own error code.

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

It starts earlier than the first session: the lines marked `setup` say when
the configuration was written and the logon task registered, whether you
typed the command or the installer did it. `init`, `install-task`,
`uninstall-task` and `stop` print those same lines as they run.

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

## Which build is this?

```bash
gamemode-executor --version
```

```
gamemode-executor 0.1.0 (de538e33)
commit:        de538e33ac376c062064131c8bbb5bcf2444ac82
repository:    https://github.com/Geeooff/GameModeExecutor
documentation: https://github.com/Geeooff/GameModeExecutor/blob/de538e33.../docs/getting-started.md
```

The first question to ask of a machine that is not yours. The commit is the
exact revision these executables were built from, and the documentation link
goes to this page **as it was at that commit** — so it describes the program in
front of you rather than whatever the project looks like today.

`gamemode-executorw.exe --version` says the same thing, and `status` prints it
too.

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

**The icon is red, with a slash.**
The configuration file cannot be used, and the first line of the icon's menu
says why — a line number and what the parser found there, or *the file is
missing*. Nothing runs until it is fixed: **Edit configuration** opens the
file, and the moment a usable one is saved the icon is grey again and the
log says `Configuration reloaded`. `gamemode-executor validate` tells the
same story in a terminal, with the parser's full account.

**I logged off during a game and the fans stayed loud.**
They calm down at your next logon. Windows does not let the stop commands run
once the session is ending, so the watcher runs them the moment it starts
again — see [How it works](how-it-works.md#logging-off-mid-game).

**The update failed, the menu says so.**
Read the log: the `update` lines carry the reason. A download that did not
verify is deleted, and a check that could not reach GitHub is just that —
try again later. If Windows Installer refused with a code, the log names it
and `%LOCALAPPDATA%\GameModeExecutor\updates\install.log` has its own
account. The version you had keeps running either way.

**The fans take ages to calm down after I quit.**
That wait is Windows', not this program's. It releases its own "a game is
running" signal when it decides to — sometimes in seconds, sometimes in minutes,
and not predictably per game. There is nothing to tune. The log shows exactly
where the time went if you set `log_level = "debug"`.

## Turning it off

```bash
gamemode-executor stop
gamemode-executor uninstall-task
```

The first stops the one running now; the second removes the logon task, so
nothing starts at the next logon. To take the program off the machine, see
[Removing it](how-it-works.md#removing-it): *Programs and Features* for the
installer's copy, or `purge` for every trace whichever way it came.

## Everything else

The [reference](reference.md) lists every command, every configuration field
and every exit code. [How it works](how-it-works.md) explains the mechanism
for the curious.
