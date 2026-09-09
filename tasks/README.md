# Scheduled task templates

Task Scheduler definitions for programs that GameModeExecutor cannot start
itself.

The watcher runs unelevated on purpose. A program whose manifest declares
`requestedExecutionLevel level="requireAdministrator"` therefore cannot be
started from it: `CreateProcess` fails with error 740,
`ERROR_ELEVATION_REQUIRED`. **FanControl is such a program**, because it talks to
hardware.

The bridge is a task registered once with *run with highest privileges*.
Triggering it needs no elevation and raises no UAC prompt, so the action in your
configuration becomes:

```toml
[[on_game_start]]
program = "schtasks.exe"
args = ["/Run", "/TN", "GameModeExecutor - FanControl Game"]
```

See the README section
[Programs that require elevation](../README.md#programs-that-require-elevation).

## Files

| File | Applies |
| --- | --- |
| `FanControl-Game.xml` | the FanControl `Game.json` profile |
| `FanControl-Quiet.xml` | the FanControl `Quiet.json` profile |

Both are UTF-16 with a BOM, the encoding Task Scheduler itself exports.

## Before importing

These are templates. Replace two placeholders in each file:

| Placeholder | Replace with |
| --- | --- |
| `__DOMAIN__\__USERNAME__` | your account, e.g. `MYPC\me` — `whoami` prints it |
| `__FANCONTROL_DIR__` | the folder holding `FanControl.exe` |

`__FANCONTROL_DIR__` appears twice: in `<Command>` and in `<WorkingDirectory>`.

## Importing

Registering a task that runs with highest privileges **requires an elevated
prompt**. Without one, `schtasks` reports `Access is denied`.

From an elevated PowerShell or Command Prompt:

```
schtasks /Create /XML "FanControl-Game.xml"  /TN "GameModeExecutor - FanControl Game"  /F
schtasks /Create /XML "FanControl-Quiet.xml" /TN "GameModeExecutor - FanControl Quiet" /F
```

Or open Task Scheduler as administrator and use **Action → Import Task**.

Check what landed:

```powershell
Get-ScheduledTask -TaskName 'GameModeExecutor - FanControl *' |
  Select-Object TaskName, @{n='RunLevel';e={$_.Principal.RunLevel}}
```

## Why these settings

| Setting | Value | Reason |
| --- | --- | --- |
| `<Triggers />` | empty | The task must only ever run when something asks. A trigger would apply a fan profile on its own. |
| `RunLevel` | `HighestAvailable` | The whole point: this is what runs the program elevated without a prompt. |
| `LogonType` | `InteractiveToken` | FanControl is a desktop application and belongs in your interactive session. |
| `MultipleInstancesPolicy` | `Parallel` | If FanControl was not already running, the task's own process *is* FanControl and stays alive. `IgnoreNew` would then silently drop every later request. |
| `ExecutionTimeLimit` | `PT0S` (none) | Same reason inverted: a time limit would eventually kill FanControl itself. |
| `AllowHardTerminate` | `false` | Keeps the scheduler from force-killing a FanControl it happens to own. |
| `DisallowStartIfOnBatteries`, `StopIfGoingOnBatteries` | `false` | Otherwise nothing happens on a laptop running on battery. |

## Adapting them to something else

Nothing here is FanControl-specific beyond `<Command>` and `<Arguments>`. Any
program that needs elevation can use the same shape: copy a file, change those
two elements and the description, register it under a new name, and point an
action at it with `schtasks /Run`.
