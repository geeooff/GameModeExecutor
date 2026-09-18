# Fan profiles with FanControl

**The goal.** FanControl sits on your everyday fan curves normally, and
switches to your gaming ones the moment Windows sees a game start — then back
when you stop.

FanControl calls a saved set of fan curves a **configuration** — that is the
word its menus use and the name of the folder they land in — and so does this
guide. To keep the two apart, *this program's* `config.toml` is always called
the configuration **file**.

**Two roles, your names.** The watcher only knows two situations, *Idle* (no
game) and *Game*, and it triggers a scheduled task named after each:
`FanControl Idle` and `FanControl Game`. Which of *your* FanControl
configurations each task applies is decided when you register the tasks, and
lives there. Call them `Quiet` and `Game`, `Silent` and `Loud`, whatever you
like — the configuration file never has to know.

In this folder:

| File | What it is |
| --- | --- |
| [`config.toml`](config.toml) | the complete configuration, ready to copy — the same for everyone |
| [`install-tasks.ps1`](install-tasks.ps1) | registers both tasks for you, asking which configuration plays which role |
| [`uninstall-tasks.ps1`](uninstall-tasks.ps1) | removes those two tasks again, and nothing else |
| [`elevate.ps1`](elevate.ps1) | shared by the two scripts: asks for administrator rights so you need not open an elevated window |
| [`FanControl-Idle.xml`](FanControl-Idle.xml) | Task Scheduler definition for the *Idle* role |
| [`FanControl-Game.xml`](FanControl-Game.xml) | the same for *Game* |

This one takes a detour, and it is worth understanding why before you start.

## Why it is not a one-liner

FanControl talks to your hardware, so it declares in its manifest that it
requires administrator rights. This watcher runs **without** administrator
rights, on purpose, and Windows will not let a program without them start one
that needs them: the attempt fails with error `740`,
`ERROR_ELEVATION_REQUIRED`.

The way round is a **scheduled task** for each role, registered once with
*run with highest privileges*. Asking Task Scheduler to run a task needs no
rights at all and shows no prompt. So the watcher triggers tasks, and the tasks
run FanControl.

You do this once. After that it is invisible.

## 1. Check FanControl's side

You need two configurations saved in FanControl: one for everyday use and one
for games. Their names are yours — this guide uses `Quiet` and `Game` as its
example, and you will be asked for the real ones.

Create them in FanControl's interface if you have not already — set the curves
you want, then **Save configuration as…**. They land in the `Configurations`
folder next to `FanControl.exe` — for the installer,
`C:\Program Files (x86)\FanControl\Configurations`, service or not. A fresh
FanControl has one configuration already, `userConfig`; that is your everyday
one until you rename it, and it is fine to use it as such.

## 2. Find where FanControl actually lives

FanControl comes two ways, and they end up in different places:

- **The installer** puts it in `C:\Program Files (x86)\FanControl` and offers
  to run it **as a Windows service**. Both are fine here. `-c` reaches the
  service exactly as it reaches the windowed application, and needs the same
  elevation either way — so nothing below changes for a service install.
- **The archive** is extracted wherever you like, so there is no path to
  assume.

This finds it:

```powershell
@(
  (Get-Process FanControl -ErrorAction Ignore | Select-Object -First 1).Path,
  "${env:ProgramFiles(x86)}\FanControl\FanControl.exe",
  "$env:ProgramFiles\FanControl\FanControl.exe",
  "$env:LOCALAPPDATA\Programs\FanControl\FanControl.exe",
  "$env:USERPROFILE\scoop\apps\fancontrol\current\FanControl.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1 | Split-Path
```

If it prints nothing, you have a portable copy somewhere of your own choosing —
right-click your FanControl shortcut and pick **Open file location**.

Keep that folder path. Everything below calls it **the FanControl folder**.

## 3. Create the two tasks

Each task runs FanControl with `-c` and a configuration name. That flag does
exactly what is needed here: it applies the configuration, and if FanControl is
*already* running it switches the live one instead of starting a second copy.
With FanControl installed as a service, the same command hands the
configuration to the service and exits at once.

| Task | Runs | Applied |
| --- | --- | --- |
| `FanControl Game` | `FanControl.exe -c Game.json` | when a game starts |
| `FanControl Idle` | `FanControl.exe -c Quiet.json` | when it stops |

— with `Game.json` and `Quiet.json` standing for *your* configuration names.

Registering a task that runs with highest privileges **needs administrator
rights, once**. Without them the registration is refused with `Access is
denied`.

### The quick way: let the script do it

[`install-tasks.ps1`](install-tasks.ps1) fills the placeholders in and registers
both tasks. From any PowerShell window, in this folder:

```powershell
.\install-tasks.ps1
```

It asks for administrator rights itself — one prompt. On Windows 11 with
`sudo` enabled in *inline* mode it carries on in the same window; otherwise
a second window opens for the elevated part and waits for Enter before
closing. Refuse the prompt and nothing is changed.

It finds FanControl by itself, lists the configurations you have saved, and
asks which one plays each role:

```
FanControl     : C:\Program Files (x86)\FanControl
Configurations :
   1) Benchmark
   2) Game
   3) Quiet
Active now     : Quiet

Configuration for Idle -- applied when no game is running
  number or name [Quiet]:

Configuration for Game -- applied while a game is running
  number or name [Game]:
```

Answer with a number from the list or a name as saved; the defaults in
brackets are taken with Enter. For *Idle* the default is the configuration
FanControl is applying right now — nobody runs this in the middle of a game,
so that is the everyday one; for *Game*, a configuration named `Game` if you
saved one. Anything that is not a saved configuration is refused and asked
again, and the list is read afresh each time, so you can switch to FanControl,
save the configuration you are missing, and come back to type its name. To
skip the questions, or if it cannot find FanControl, say so:

```powershell
.\install-tasks.ps1 -IdleConfiguration Quiet -GameConfiguration Game
.\install-tasks.ps1 -FanControlDir "D:\Tools\FanControl"
```

A name given this way that is not saved is refused too, with nothing
registered. At the end it prints the commands to test what it registered.
Run it again any time you rename a configuration: the tasks are simply
re-registered.

### Or by hand: import the templates

Open [`FanControl-Idle.xml`](FanControl-Idle.xml) and
[`FanControl-Game.xml`](FanControl-Game.xml) in a text editor and replace three
placeholders in each:

| Placeholder | Replace with |
| --- | --- |
| `__DOMAIN__\__USERNAME__` | your account — run `whoami` to print it |
| `__FANCONTROL_DIR__` | the FanControl folder (it appears **twice** per file) |
| `__CONFIGURATION__` | the configuration for that role — `Quiet` in the Idle file, `Game` in the Game file, or whatever yours are called |

Both files are UTF-16 with a BOM, the encoding Task Scheduler itself exports —
keep it if your editor asks.

Then, from a PowerShell or Command Prompt **opened as administrator**, in this
folder:

```
schtasks /Create /XML "FanControl-Idle.xml" /TN "GameModeExecutor\FanControl Idle" /F
schtasks /Create /XML "FanControl-Game.xml" /TN "GameModeExecutor\FanControl Game" /F
```

The backslash in the name puts the task inside a **`GameModeExecutor` folder**
in Task Scheduler rather than loose at the root, next to Windows' own. The
folder is created on demand; nothing has to make it first.

Check what landed:

```powershell
Get-ScheduledTask -TaskPath '\GameModeExecutor\' |
  Select-Object TaskName, @{n='RunLevel';e={$_.Principal.RunLevel}}
```

### Or by hand, in Task Scheduler

Press `Win+R`, type `taskschd.msc`, and run it **as administrator**.

First make the folder, once: right-click **Task Scheduler Library** in the left
pane → **New Folder…** → `GameModeExecutor`. Then select that folder, so what
you create next lands inside it rather than at the root.

Then **Action → Create Task…** — not *Create Basic Task*, which does not offer
the settings that matter.

Do this twice, once per role. For `Game`:

**General tab**
- Name: `FanControl Game` — the folder already says which program it belongs to
- ☑ **Run with highest privileges** ← this is the entire point of the detour
- Leave *Run only when user is logged on* selected

**Triggers tab**
- **Nothing.** Add no trigger at all. A trigger would apply a configuration on
  its own; this task must only ever run when asked.

**Actions tab** → New…
- Action: *Start a program*
- Program: the full path to `FanControl.exe`
- Add arguments: `-c Game.json` — your gaming configuration's file name
- **Start in**: the FanControl folder — without it, FanControl will not find the
  configuration

**Conditions tab**
- ☐ Uncheck **Start the task only if the computer is on AC power**
- ☐ Uncheck **Stop if the computer switches to battery power**

  *Otherwise nothing happens on a laptop running on battery.*

**Settings tab**
- ☐ Uncheck **Stop the task if it runs longer than**
- ☐ Uncheck **If the running task does not end when requested, force it to stop**
- At the bottom, *If the task is already running…*: choose **Run a new instance
  in parallel**

  *These three matter for one reason: if FanControl was not already running, the
  task's own process **is** FanControl and stays alive. With the defaults, Task
  Scheduler would eventually kill it, and would silently ignore every later
  request to switch configuration. With the service, the task's process exits at once
  and these settings never come into play — they cost nothing, and keep the
  task right for both ways of running FanControl.*

Then repeat, changing only the name to `FanControl Idle` and the argument to
your everyday configuration — `-c Quiet.json` in this guide's example.

## 4. Test the tasks on their own

Before involving any game. From a normal, **non**-administrator prompt:

```powershell
schtasks /Run /TN "GameModeExecutor\FanControl Game"
schtasks /Run /TN "GameModeExecutor\FanControl Idle"
```

Watch FanControl's window: the active configuration should change each time. If
this does not work, nothing further will, and the problem is on this side.

## 5. The configuration

Copy [`config.toml`](config.toml) over your own. It is the complete file:

```toml
[general]
stop_actions_on_exit = true
log_level = "info"

[on_game_start]
mode = "series"

[[on_game_start.actions]]
name = "FanControl - Game"
program = "schtasks.exe"
args = ["/Run", "/TN", 'GameModeExecutor\FanControl Game']
wait = true
timeout = "15s"

[on_game_stop]
mode = "series"

[[on_game_stop.actions]]
name = "FanControl - Idle"
program = "schtasks.exe"
args = ["/Run", "/TN", 'GameModeExecutor\FanControl Idle']
wait = true
timeout = "15s"
```

Note there is no path to FanControl anywhere in it, and no configuration
name either. That is the point of the detour: both live in the task, so this
file is the same for everyone and stays something a non-administrator can edit
freely.

```bash
gamemode-executor validate
```

## 6. Test the chain, then turn it on

```bash
gamemode-executor trigger start   # should switch FanControl to your gaming configuration
gamemode-executor trigger stop    # and back to the everyday one
```

You can confirm it from FanControl's own state rather than from our log —
FanControl records the active configuration in a file called `CACHE`, in its
`Configurations` folder:

```powershell
(Get-Content "<the FanControl folder>\Configurations\CACHE" -Raw |
  ConvertFrom-Json).CurrentConfigFileName
```

When both work, restart the watcher so it reads the file:

```bash
gamemode-executor stop
gamemode-executor install-task
```

From the zip, the second line also registers the logon task, the first time.
Done. Play a game and the fans follow.

## Worth knowing

**FanControl must be running** for a switch to apply to a live system.
If it is not, the task starts it — which works, but is slower. Most people have
it start with Windows.

**Nothing happens at logon.** There is no "no game" event when the watcher
starts, so FanControl keeps whatever configuration it had. Since it remembers
its last one across restarts, and the watcher applies the idle configuration
whenever a session ends, it settles correctly on its own.

**The switch back can lag**, sometimes by a minute or more. That wait is Windows
releasing its own "a game is running" signal, not this program —
[How it works](../../how-it-works.md#the-wait-after-you-quit) explains it, and
there is nothing to tune.

## Removing the recipe

From any PowerShell window, in this folder — it asks for administrator rights
the same way the install script does:

```powershell
.\uninstall-tasks.ps1
```

It removes the two tasks it knows — *FanControl Idle* and *FanControl Game* —
and leaves everything else: FanControl and its configurations, the
`\GameModeExecutor` folder in Task Scheduler with the watcher's own task in
it, and any task it did not register, which it lists. FanControl keeps
whichever configuration is active at that moment; pick the one you want in
FanControl itself.

Then edit `config.toml`: it still names the two tasks, and the watcher would
report a failed command at the next game. Replace the commands with whatever
you want run instead, or start over from another recipe.

GameModeExecutor does not know which recipe you followed, so removing the
program never touches these tasks; this script is how they go.

## Adapting this to another program

Nothing here is FanControl-specific beyond `<Command>` and `<Arguments>` in the
task files. Any program needing administrator rights can use the same shape:
copy a template, change those two elements and the description, register it
under a new name, and point an action at it with `schtasks /Run`.

The settings to keep, and why:

| Setting | Value | Reason |
| --- | --- | --- |
| `<Triggers />` | empty | The task must only run when something asks. A trigger would fire it on its own. |
| `RunLevel` | `HighestAvailable` | The whole point: runs the program elevated without a prompt. |
| `LogonType` | `InteractiveToken` | A desktop application belongs in your interactive session. |
| `MultipleInstancesPolicy` | `Parallel` | If the program was not already running, the task's process *is* it. `IgnoreNew` would silently drop every later request. |
| `ExecutionTimeLimit` | `PT0S` | Same reason inverted: a limit would eventually kill the program. |
| `AllowHardTerminate` | `false` | Keeps the scheduler from force-killing a program it happens to own. |
| battery settings | `false` | Otherwise nothing happens on a laptop on battery. |
