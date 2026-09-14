# Writing your own

Reference rather than a recipe: every option a command takes, and every
placeholder you can put in one.

In this folder: [`config.toml`](config.toml), annotated, showing all of it in
one place. It is a working file — every command in it is harmless — so you can
copy it and delete what you do not want.

## The shape

A configuration has two events, and each holds a list of commands:

```toml
[on_game_start]
mode = "series"

[[on_game_start.actions]]
program = "..."

[[on_game_start.actions]]
program = "..."

[on_game_stop]
mode = "series"

[[on_game_stop.actions]]
program = "..."
```

`[[double brackets]]` add another command to the list. Repeat as many as you
like.

## What a command accepts

| Key | Default | What it does |
| --- | --- | --- |
| `program` | *required* | The executable. A bare name is looked up on `PATH`. |
| `args` | none | Its arguments, **one string per argument** — not one string with spaces. |
| `name` | the program path | Label used in the log. Worth setting, it is what you read later. |
| `working_dir` | inherited | Directory to run it from. |
| `env` | none | Extra environment variables, as `KEY = "value"`. |
| `no_window` | `true` | Hide the command's own console window. |
| `wait` | `false` | Wait for it to finish before moving on. |
| `timeout` | none | Give up waiting after this long. Ignored unless `wait`. |
| `enabled` | `true` | Set `false` to keep a command in the file without running it. |

### On paths

Write them between **single quotes**. TOML takes those literally, so Windows
backslashes need no doubling:

```toml
program = 'C:\Program Files\Something\tool.exe'   # good
program = "C:\\Program Files\\Something\\tool.exe" # also correct, but tiresome
```

### On arguments

One string per argument:

```toml
args = ["--mode", "gaming"]     # good
args = ["--mode gaming"]        # wrong: one argument containing a space
```

## Series or parallel

```toml
[on_game_start]
mode = "series"     # each command runs after the previous one finished. Default.
mode = "parallel"   # all started at once, then waited for.
```

`series` is the right default. Use `parallel` when one command is slow and the
others do not depend on it.

A command that fails to start is logged and never prevents the others from
running.

## Placeholders

These are substituted in `program`, `args`, `working_dir` and `env`:

| Placeholder | Becomes |
| --- | --- |
| `{event}` | `game_start` or `game_stop` |
| `{process_name}` | e.g. `bf6.exe` |
| `{process_id}` | e.g. `14552` |
| `{process_path}` | the full path, when readable |

```toml
args = ["-Command", "Write-Host 'now playing {process_name}'"]
```

They are a convenience, filled from the list of titles Windows itself keeps. A
game Windows tracks but does not describe leaves them empty — **detection never
depends on them**, and every command still runs.

`{process_id}` is deliberately empty on `game_stop`: by then that process is
usually long gone, and claiming otherwise would be a lie.

## The rest of the file

```toml
[general]
stop_actions_on_exit = true   # run the stop commands if the watcher is stopped
                              # mid-game -- logoff and shutdown included
log_level = "info"            # error | warn | info | debug | trace
#log_dir = 'C:\somewhere\logs'  # defaults to %APPDATA%\GameModeExecutor\logs

[detection]
poll_interval = "2s"     # how often to look for a game while none is running.
                         # The only polling this program ever does.
stop_delay = "2s"        # grace period after the signal drops, in case Windows
                         # briefly brings it back mid-session
identify_after = "20s"   # when to ask the GPU which matched process is really
                         # the game. "0s" skips it.
gpu_sample = "1s"        # GPU load is a rate, so it needs two readings this
                         # far apart
```

Every one of these has a sensible default. You can leave the whole `[detection]`
block out.

## Checking your work

```bash
gamemode-executor validate          # points at the line and column of a mistake
gamemode-executor trigger start     # run the start commands now, no game needed
gamemode-executor trigger stop
```

## If a program needs administrator rights

It cannot be started from here — see
[Fan profiles with FanControl](../fancontrol-fan-profiles/), which is that case
worked through end to end.
