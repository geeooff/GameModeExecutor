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

- **No game:** it looks every two seconds. That is the only polling it ever
  does, and it is cheap: it asks Windows for the list of process numbers
  alone, which takes a few hundredths of a millisecond, and asks a process
  its name only the first time it sees it — a full list of every process by
  name, which costs a hundred times more, only every thirty seconds.
- **Game running:** it stops looking entirely, and asks Windows to wake it when
  that process ends. Zero activity while you play — which is rather the point of
  a program that runs during games.

The result is that anything Windows knows as a game triggers it, including
titles released after this program was written — Windows keeps its list of
games up to date on its own.

**Games you marked yourself.** A title Windows does not recognise, you can
teach it: open the Game Bar over it and tick *Remember this is a game*.
Windows treats it as a game from then on — overlay, capture, Game Mode — but
does not start the process above for it, because that process exists to
tell Xbox what you are playing, and a title you named by hand has no Xbox
identity to tell. So the watcher also reads Windows' own list of the games
you marked: when one of those executables runs, that is a game session, with
the same commands at both ends. It is the same two-second look, and the
list is read again only when you tick or untick a box, so ticking it in the
middle of a game starts the session within those two seconds — nothing to
relaunch. While that game runs, the watcher waits on the game itself, and
does nothing else. The log names such a game as one *marked as a game by
hand*: a program ticked by mistake — a browser, say — would be a game
session whenever it runs, and that line is how you find the box to untick.

**Boxes you no longer need.** Microsoft adds games to its list after they
are released, and a box you ticked before that keeps Windows on your own
entry rather than its own — measured on 2026-09-23 with *Death Stranding 2*
and *Wreckfest 2*, both listed by Microsoft by then. Each time the watcher
starts, it compares the games you marked with Microsoft's list and says, in
the log, which ones Microsoft now knows. For those, untick *Remember this is
a game*, quit the game and start it again: Windows recognises it by itself
— the Xbox overlay shows you *playing* it — and so does this program.
`gamemode-executor status` lists the games you marked and says the same.

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

The commands for "game stopped" can fire well after you have quit.

That wait is Windows'. It keeps its "a game is running" signal until you next
touch the mouse or the keyboard after the game has closed, then lets go about
twenty seconds later. Keep using the PC and it takes seconds; quit and walk
away, and the commands wait for your return — two hours, once. No setting
changes that. This program adds about two seconds, a grace period in case
Windows briefly restarts the signal mid-session.

Set `log_level = "debug"` and the log says when Windows let go and whether the
game had already exited.

*Corrected 2026-09-25: this page called the wait unpredictable.*

## What it costs your machine

Measured on the whole watcher, as Windows counts it, on a gaming PC in
September 2026. Memory here is private memory — Task Manager's *Commit
size* column in *Details*; its *Memory* column shows less.

| When | Processor | Memory |
| --- | --- | --- |
| Waiting for a game, looking every two seconds | 0.03 % of one core | about 2 MB |
| During a game | nothing measurable: it waits for Windows to wake it | unchanged |
| Naming a game from what the graphics card draws, once a session | | about 0.25 MB, once |
| The icon's menu, the first time it opens | | about 1 MB, once — Windows' cost for a program's first menu |
| *Edit configuration*, *Open log*, *Documentation* | | nothing that stays: a short-lived helper opens them and takes the cost with it |
| *Check for updates* | | about 1 MB, until the watcher next starts |

Over one evening — a Battlefield 6 session, five hours of GTA Online, the
menu used, and the rest idle, six hours in all — the watcher used 1.25
seconds of processor time.

## Why there are two executables

Windows makes a program choose, when it is built, between two kinds:

- a **console** program, which owns a black window and can print into it;
- a **windowless** program, which has nowhere to print at all.

It is not a setting you flip afterwards. So a program that must both sit
silently in the background *and* answer you when you type commands cannot be one
file.

Hence the pair. Both understand the same commands. `gamemode-executor.exe`
is the one you type them into, because it answers where you can read it and
a shell waits for it to finish. `gamemode-executorw.exe` prints nothing and
nobody waits for it, which is exactly right for the two things that run it:
the logon task, which runs the watcher, and the installer, which writes your
starter configuration and registers the task through it — a console program
would flash a black window in the middle of the install. What it did is
written in the log instead. Python solves the same problem the same way, with
`python.exe` and `pythonw.exe`.

The alternative — one windowless program that borrows the terminal it was
launched from — was tried on paper and rejected: a shell does not wait for a
windowless program, so the prompt comes back before the output, and commands
like `validate` would stop reporting success or failure to any script using
them. Silently, which is the worst way for that to break.

Both files are built from the same code, so the watcher they run is identical,
and so is every command.

## Only one watcher at a time

A named lock makes sure of it. If you run `gamemode-executor run` while the
background one is going, the second one refuses and exits with a distinct code
rather than doubling up your commands.

## What starts it, and why not a service

A **per-user scheduled task**, triggered at logon, fifteen seconds in. The
installer registers it and starts it once the files are in place, which is
why the icon appears as the install ends; from the zip, `install-task` does
the same by hand. Before an upgrade or an uninstall replaces or removes the
executables, the installer runs `stop` — *Quit*, typed — so Windows never
finds the watcher holding a file it is about to touch and never has to ask
you to close it.

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

The same file lets one watcher hand a game to the next. When the watcher is
stopped for an update — by the installer, or by `stop --handover` — with a
game on, it runs nothing and leaves the file saying the session is open; the
watcher that starts a second later finds the game still running and takes
the session up where it was, icon and name included. Nothing runs twice, and
your gaming configuration is never switched off and on again in the middle
of a game. If the game ended in that second, the new watcher runs the stop
commands instead, as after a logoff.

`gamemode-executor status` shows whether that file is there, and where.

## Changing the configuration

The watcher does not read the file once and forget it. A small thread waits
on the folder's change notification — Windows' own, nothing polled — and
when the file's bytes change it stops the running engine and starts one on
the new file. Editors save in several steps, so the read waits a quarter of
a second after the last change; the log then says `Configuration reloaded`.
A file written back unchanged is not a reload.

A game in progress survives it, by the same handover an update uses: the
engine stops without running anything, leaving the session open in the
[marker](#logging-off-mid-game), and the next engine finds the presence
writer still running and takes the session up where it was. The stop
commands that run when the game ends are the new ones.

A file that cannot be used — a typo, a value the program refuses, a file
that is gone — does not stop the program and does not fall back to the last
good one. It **disables it outright**: the icon turns red, a notification
says what is wrong, the menu's first line keeps the short of it, and
nothing is watched until a usable file is saved; when one is, a
notification says the watcher is back. Frozen, deliberately. Falling back
to the last good configuration
would mean the program runs something other than what the file says, with
nothing on screen to say so; frozen with a red icon is an unambiguous
state, and *Edit configuration* is right there. While it is frozen a game
that ends gets no stop commands, and the session marker remembers that: the
moment a usable file is saved, the new engine settles what was left — the
stop commands run if the game is gone, the session resumes if it is still
on. `log_dir` is the one setting a reload cannot apply, because the log is
already open; it takes effect at the next start, and the log says so.

## Removing it

Uninstalling from *Programs and Features* removes what the installer put
there — the executables and the logon task it registered, which would
otherwise try to start a missing program at every logon — and nothing else,
as Windows applications ordinarily do: your configuration, the log and the
session marker stay, so that installing again finds everything as you left
it. Upgrading touches none of them, the task included.

When you want every trace gone, ask for it:

```
gamemode-executor purge
```

It lists what it is about to remove and waits for a `yes`: the logon task,
the configuration wherever it found it, the log wherever it was written, the
session marker, its two profile folders once they are empty, and last the
executables — through Windows Installer when they were installed from the
package, or by deleting them once the command has exited when they were
unpacked from the zip. It refuses while a game is running, because a purge
then would leave your gaming configuration on with nothing left to restore
it; and it stops the running watcher first, the way *Quit* does.

It removes only what it recognises as its own. A scheduled task it did not
register — the ones a recipe had you create, say — stays, and so does the
`\GameModeExecutor` folder in Task Scheduler around it; a folder that holds
anything else stays too. The recipes carry their own way out for what they
added.

## Updating itself

*Check for updates* asks GitHub for the latest release — one request, no
API, no key — and compares the tag with the version running. If it is
newer, *Download and install* fetches the installer (or the zip, for a
copy unpacked by hand) by its tag, computes its SHA-256 with Windows' own
cryptography and compares it with the `SHA256SUMS.txt` the release
publishes; a file that does not match is deleted before anything can run
it. Then the installer is the updater: the package is run quietly, stops
the watcher with a handover, replaces the files and starts the new version,
which resumes the game session if there was one. An unpacked copy does the
same through a small hidden shell that waits for the watcher to exit,
expands the archive over the folder — keeping the previous executables as
`.old` until the new version has started — and runs `install-task`.

The hash proves the file is the one the release published, not that the
release is honest; the program has no code signature, and the design record
says why. A file it downloads carries no mark of the web, so Windows'
SmartScreen never sees it: the program vouches for it, through the hash.

The notification that answers *Check for updates* is one of two times the
program shows anything beyond its icon: a menu closes when you click in it,
so the answer has to reach you somewhere. The other is a configuration that
cannot be used, and its end — the one thing that needs you. Both are
silent and respect your quiet hours; nothing else is ever said.

## What it does not do

- **No network it did not ask you about.** It connects to exactly one
  thing, GitHub, and only when you click *Check for updates* or run
  `update`. There is no telemetry, nothing is polled, and the check itself
  is one request: where does `releases/latest` redirect — the tag is the
  answer.
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
| The program | `%LOCALAPPDATA%\Programs\GameModeExecutor` from the installer — the Windows convention for a per-user install, and it stays writable, which `C:\Program Files` would not. From the zip, wherever you put it |
| Configuration | `config.toml` next to the executable if there is one, otherwise `%APPDATA%\GameModeExecutor\config.toml` |
| Log | `%LOCALAPPDATA%\GameModeExecutor\logs\gamemode-executor.log`, one file, local timestamps |
| Scheduled tasks | a `GameModeExecutor` folder in Task Scheduler, holding `Watcher` and anything a recipe added |
| A release being installed | `%LOCALAPPDATA%\GameModeExecutor\updates\`, emptied once the new version has started |

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
