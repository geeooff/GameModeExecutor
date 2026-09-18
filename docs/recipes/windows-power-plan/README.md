# Windows power plan

**The goal.** High performance while you play, balanced the rest of the time.

This is what most recipes look like: no elevation, no scheduled tasks, no
detour. `powercfg.exe` ships with Windows and runs perfectly well as you.

In this folder: [`config.toml`](config.toml), the complete configuration.

## 1. Find out which plans you have

```powershell
powercfg /list
```

You get something like:

```
Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced) *
Power Scheme GUID: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (High performance)
```

The `*` marks the active one.

Windows accepts these aliases in place of the GUIDs, which is what the
configuration uses:

| Alias | Plan |
| --- | --- |
| `SCHEME_MAX` | Power saver |
| `SCHEME_BALANCED` | Balanced |
| `SCHEME_MIN` | High performance |

If a plan you want has no alias — an Ultimate Performance plan, or one from your
laptop's manufacturer — use its GUID from the list above instead.

## 2. The configuration

Copy [`config.toml`](config.toml) over your own:

```toml
[general]
stop_actions_on_exit = true
log_level = "info"

[on_game_start]
mode = "series"

[[on_game_start.actions]]
name = "power plan - high performance"
program = "powercfg.exe"
args = ["/setactive", "SCHEME_MIN"]
wait = true
timeout = "10s"

[on_game_stop]
mode = "series"

[[on_game_stop.actions]]
name = "power plan - balanced"
program = "powercfg.exe"
args = ["/setactive", "SCHEME_BALANCED"]
wait = true
timeout = "10s"
```

## 3. Check it

```bash
gamemode-executor validate
gamemode-executor trigger start
```

Then confirm from Windows rather than from our log:

```powershell
powercfg /getactivescheme
```

```bash
gamemode-executor trigger stop
```

When both work, restart the watcher so it reads the file:

```bash
gamemode-executor stop
gamemode-executor install-task
```

From the zip, the second line also registers the logon task, the first time.

## Worth knowing

**On a laptop, Windows may override you.** Some machines switch plans by
themselves when the power source changes, or when a manufacturer utility decides
to. If the plan does not stick, that is usually the cause, and it is outside
this program.

**You can combine recipes.** Nothing stops you adding the fan commands from
[Fan profiles with FanControl](../fancontrol-fan-profiles/) to the same events —
list the actions one after another and they run in order.
