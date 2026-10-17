# Fan profiles with FanControl

FanControl stays on your everyday fan curves, and switches to your gaming ones
while a game runs.

FanControl is Rémi Mercier's work: [getfancontrol.com](https://getfancontrol.com/),
releases on [GitHub](https://github.com/Rem0o/FanControl.Releases). It calls a
saved set of curves a **configuration**; this program's `config.toml` is called
the configuration **file** here.

## Why a detour

FanControl requires administrator rights, and the watcher never has them:
Windows refuses the start with error 740. So each role gets a **scheduled
task** registered once *with highest privileges*; running a task needs no
rights and shows no prompt. The watcher runs `FanControl Game` when a game
starts and `FanControl Idle` when it stops. Which of your configurations each
applies is set in the task, so `config.toml` is the same for everyone.

| File | |
| --- | --- |
| [`config.toml`](config.toml) | the configuration file, ready to copy |
| [`install-tasks.cmd`](install-tasks.cmd) / [`.ps1`](install-tasks.ps1) | registers both tasks, asking which configuration plays which role |
| [`uninstall-tasks.cmd`](uninstall-tasks.cmd) / [`.ps1`](uninstall-tasks.ps1) | removes those two tasks, nothing else |
| [`elevate.ps1`](elevate.ps1) | asks for administrator rights for the two scripts |
| [`FanControl-Idle.xml`](FanControl-Idle.xml), [`FanControl-Game.xml`](FanControl-Game.xml) | the task definitions |

## 1. Two configurations in FanControl

Save one for everyday use and one for games (**Save configuration as…**).
Names are yours; this page says `Quiet` and `Game`. A fresh FanControl's
`userConfig` can be the everyday one. FanControl may run as an application or
as a service; both work the same here.

## 2. Register the tasks

**Double-click `install-tasks.cmd`.** It runs the script even where Windows
refuses scripts, asks for administrator rights once, finds FanControl, lists
your configurations and asks which plays each role:

```
FanControl     : C:\Program Files (x86)\FanControl
Configurations :
   1) Benchmark
   2) Game
   3) Quiet
Active now     : Quiet

Configuration for Idle -- applied when no game is running
  number or name [Quiet]:
```

Enter takes the default. If Windows warns about a file from the internet,
allow it, or *Unblock* the zip in its *Properties* first. Run it again after
renaming a configuration. To skip the questions or point at FanControl:

```powershell
.\install-tasks.ps1 -IdleConfiguration Quiet -GameConfiguration Game
.\install-tasks.ps1 -FanControlDir "D:\Tools\FanControl"
```

**By hand instead:** in both XML files replace `__DOMAIN__\__USERNAME__`
(`whoami`), `__FANCONTROL_DIR__` (twice) and `__CONFIGURATION__`. Keep the
files UTF-16. Then, as administrator, in this folder:

```
schtasks /Create /XML "FanControl-Idle.xml" /TN "GameModeExecutor\FanControl Idle" /F
schtasks /Create /XML "FanControl-Game.xml" /TN "GameModeExecutor\FanControl Game" /F
```

Each task runs `FanControl.exe -c <configuration>.json`, which switches a
running FanControl, or the service, to that configuration.

## 3. Test the tasks

From a normal prompt; FanControl's active configuration should change each
time:

```powershell
schtasks /Run /TN "GameModeExecutor\FanControl Game"
schtasks /Run /TN "GameModeExecutor\FanControl Idle"
```

## 4. The configuration file

Replace your `config.toml` with [this one](config.toml), whole. (If you already
run commands of your own, add its two actions to your file instead.) The
actions:

```toml
[[on_game_start.actions]]
name = "FanControl - Game"
program = "schtasks.exe"
args = ["/Run", "/TN", 'GameModeExecutor\FanControl Game']
wait = true
timeout = "15s"

[[on_game_stop.actions]]
name = "FanControl - Idle"
program = "schtasks.exe"
args = ["/Run", "/TN", 'GameModeExecutor\FanControl Idle']
wait = true
timeout = "15s"
```

Then `gamemode-executor trigger start` and `trigger stop` should switch
FanControl both ways. Saved, the watcher applies it at once. Play, and the fans
follow.

## Worth knowing

- **FanControl should be running.** If not, the task starts it, more slowly.
- **Nothing is applied at logon.** FanControl keeps its last configuration,
  and the watcher applies *Idle* at every session's end.
- **The switch back can lag** a minute or more: that is Windows
  ([why](../../how-it-works.md#the-wait-after-you-quit)).

## Removing the recipe

Double-click `uninstall-tasks.cmd`. It removes the two tasks and lists any
other in the folder; FanControl keeps its current configuration. Then remove
the two actions from `config.toml`, or the watcher will report them failing.
Removing GameModeExecutor never touches these tasks.

## Another program needing administrator rights

Copy a template, change `<Command>`, `<Arguments>` and the description,
register it under a new name, and run it with `schtasks /Run`. Keep these
settings:

| Setting | Value | Why |
| --- | --- | --- |
| `<Triggers />` | empty | runs only when asked |
| `RunLevel` | `HighestAvailable` | elevated without a prompt |
| `LogonType` | `InteractiveToken` | in your desktop session |
| `MultipleInstancesPolicy` | `Parallel` | if the task's process *is* the program, `IgnoreNew` drops every later request |
| `ExecutionTimeLimit` | `PT0S` | no limit to kill the program |
| `AllowHardTerminate` | `false` | the scheduler never kills it |
| battery settings | `false` | works on battery |
