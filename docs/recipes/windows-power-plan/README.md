# Windows power plan

High performance while you play, balanced the rest of the time. No elevation
and no scheduled task: `powercfg.exe` runs as you.

## 1. Your plans

```powershell
powercfg /list
```

Windows accepts these aliases instead of the GUIDs:

| Alias | Plan |
| --- | --- |
| `SCHEME_MAX` | Power saver |
| `SCHEME_BALANCED` | Balanced |
| `SCHEME_MIN` | High performance |

A plan without an alias, such as Ultimate Performance or a manufacturer's,
goes by its GUID from the list.

## 2. The configuration file

Copy [`config.toml`](config.toml) over your own. Its two actions:

```toml
[[on_game_start.actions]]
name = "power plan - high performance"
program = "powercfg.exe"
args = ["/setactive", "SCHEME_MIN"]
wait = true
timeout = "10s"

[[on_game_stop.actions]]
name = "power plan - balanced"
program = "powercfg.exe"
args = ["/setactive", "SCHEME_BALANCED"]
wait = true
timeout = "10s"
```

## 3. Check it

```bash
gamemode-executor trigger start
```

`powercfg /getactivescheme` should show High performance; `trigger stop` brings
Balanced back. Saved, the watcher applies the file at once.

## Worth knowing

- **A laptop may switch plans by itself** on a change of power source, or a
  manufacturer's utility may. That is outside this program.
- **Recipes combine:** add the [FanControl](../fancontrol-fan-profiles/)
  actions to the same events and they run in order.
