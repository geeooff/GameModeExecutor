# Fan profiles with FanControl

**The goal.** FanControl sits on its `Quiet` profile normally, and switches to
`Game` the moment Windows sees a game start — then back to `Quiet` when you
stop.

In this folder:

| File | |
| --- | --- |
| [`config.toml`](config.toml) | the complete configuration, ready to copy |
| [`FanControl-Game.xml`](FanControl-Game.xml) | Task Scheduler definition for the `Game` profile |
| [`FanControl-Quiet.xml`](FanControl-Quiet.xml) | the same for `Quiet` |

This one takes a detour, and it is worth understanding why before you start.

## Why it is not a one-liner

FanControl talks to your hardware, so it declares in its manifest that it
requires administrator rights. This watcher runs **without** administrator
rights, on purpose, and Windows will not let a program without them start one
that needs them: the attempt fails with error `740`,
`ERROR_ELEVATION_REQUIRED`.

The way round is a **scheduled task** for each profile, registered once with
*run with highest privileges*. Asking Task Scheduler to run a task needs no
rights at all and shows no prompt. So the watcher triggers tasks, and the tasks
run FanControl.

You do this once. After that it is invisible.

## 1. Check FanControl's side

You need two profiles saved in FanControl, named exactly:

- `Quiet.json`
- `Game.json`

Create them in FanControl's interface if you have not already — set the curves
you want, then **Save configuration as…**. They land in the `Configurations`
folder next to `FanControl.exe`.

## 2. Find where FanControl actually lives

**FanControl has no standard install folder.** It is distributed as an archive
you extract wherever you like, so there is no path this guide can assume — and
the several ways of installing it each end up somewhere different.

This finds it:

```powershell
@(
  (Get-Process FanControl -ErrorAction Ignore | Select-Object -First 1).Path,
  "$env:LOCALAPPDATA\Programs\FanControl\FanControl.exe",
  "$env:ProgramFiles\FanControl\FanControl.exe",
  "$env:USERPROFILE\scoop\apps\fancontrol\current\FanControl.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1 | Split-Path
```

If it prints nothing, you have a portable copy somewhere of your own choosing —
right-click your FanControl shortcut and pick **Open file location**.

Keep that folder path. Everything below calls it **the FanControl folder**.

## 3. Create the two tasks

Each task runs FanControl with `-c` and a profile name. That flag does exactly
what is needed here: it applies the profile, and if FanControl is *already*
running it switches the live configuration instead of starting a second copy.

```
FanControl.exe -c Game.json
FanControl.exe -c Quiet.json
```

Registering a task that runs with highest privileges **needs administrator
rights, once**. Without them the registration is refused with `Access is
denied`.

### The quick way: import the templates

Open [`FanControl-Game.xml`](FanControl-Game.xml) and
[`FanControl-Quiet.xml`](FanControl-Quiet.xml) in a text editor and replace two
placeholders in each:

| Placeholder | Replace with |
| --- | --- |
| `__DOMAIN__\__USERNAME__` | your account — run `whoami` to print it |
| `__FANCONTROL_DIR__` | the FanControl folder (it appears **twice** per file) |

Both files are UTF-16 with a BOM, the encoding Task Scheduler itself exports —
keep it if your editor asks.

Then, from a PowerShell or Command Prompt **opened as administrator**, in this
folder:

```
schtasks /Create /XML "FanControl-Game.xml"  /TN "GameModeExecutor\FanControl Game"  /F
schtasks /Create /XML "FanControl-Quiet.xml" /TN "GameModeExecutor\FanControl Quiet" /F
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

Do this twice, once per profile. For the `Game` one:

**General tab**
- Name: `FanControl Game` — the folder already says which program it belongs to
- ☑ **Run with highest privileges** ← this is the entire point of the detour
- Leave *Run only when user is logged on* selected

**Triggers tab**
- **Nothing.** Add no trigger at all. A trigger would apply a fan profile on its
  own; this task must only ever run when asked.

**Actions tab** → New…
- Action: *Start a program*
- Program: the full path to `FanControl.exe`
- Add arguments: `-c Game.json`
- **Start in**: the FanControl folder — without it, FanControl will not find the
  profile

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
  request to switch profile.*

Then repeat, changing only the name to `FanControl Quiet` and the argument to
`-c Quiet.json`.

## 4. Test the tasks on their own

Before involving any game. From a normal, **non**-administrator prompt:

```powershell
schtasks /Run /TN "GameModeExecutor\FanControl Game"
schtasks /Run /TN "GameModeExecutor\FanControl Quiet"
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
name = "FanControl - Game profile"
program = "schtasks.exe"
args = ["/Run", "/TN", 'GameModeExecutor\FanControl Game']
wait = true
timeout = "15s"

[on_game_stop]
mode = "series"

[[on_game_stop.actions]]
name = "FanControl - Quiet profile"
program = "schtasks.exe"
args = ["/Run", "/TN", 'GameModeExecutor\FanControl Quiet']
wait = true
timeout = "15s"
```

Note there is no path to FanControl anywhere in it. That is the point of the
detour: the command lives in the task, so this file stays something a
non-administrator can edit freely.

```bash
gamemode-executor validate
```

## 6. Test the chain, then turn it on

```bash
gamemode-executor trigger start   # should switch FanControl to Game
gamemode-executor trigger stop    # and back to Quiet
```

You can confirm it from FanControl's own state rather than from our log —
FanControl records the active profile in a file called `CACHE`, in its
`Configurations` folder:

```powershell
(Get-Content "<the FanControl folder>\Configurations\CACHE" -Raw |
  ConvertFrom-Json).CurrentConfigFileName
```

When both work:

```bash
gamemode-executor install-task
```

Done. Play a game and the fans follow.

## Worth knowing

**FanControl must be running** for a profile switch to apply to a live system.
If it is not, the task starts it — which works, but is slower. Most people have
it start with Windows.

**Nothing happens at logon.** There is no "no game" event when the watcher
starts, so FanControl keeps whatever profile it had. Since it remembers its last
profile across restarts, and the watcher restores `Quiet` whenever a session
ends, it settles correctly on its own.

**The switch back can lag**, sometimes by a minute or more. That wait is Windows
releasing its own "a game is running" signal, not this program —
[How it works](../../how-it-works.md#the-wait-after-you-quit) explains it, and
there is nothing to tune.

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
