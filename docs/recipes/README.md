# Recipes

Worked examples, start to finish. Each one is a folder: the instructions, plus
the files you need, ready to copy. Each release carries them all as
`GameModeExecutor-recipes-<version>.zip`.

If you have not set the program up yet, read
[Getting started](../getting-started.md) first.

| Recipe | What it does | Comes with |
| --- | --- | --- |
| [Fan profiles with FanControl](fancontrol-fan-profiles/) | Quiet fans outside games, a game profile while playing. The full treatment for a program that needs administrator rights. | `config.toml`, two Task Scheduler templates |
| [Windows power plan](windows-power-plan/) | High performance while playing, balanced the rest of the time. The simple shape most commands have. | `config.toml` |

Writing your own: the starter configuration's comments list every key a
command takes and every placeholder, and [the reference](../reference.md#configuration)
has the full table.

## How to use one

Each folder holds a `config.toml` that is the complete file, not a fragment.
Copy it over your own configuration, adjust the paths it calls out, and check
it:

```bash
gamemode-executor validate
gamemode-executor trigger start
gamemode-executor trigger stop
```

## Adding one

A recipe is a folder containing at least a `README.md`, and usually a
`config.toml` beside it. Add the folder, add a row to the table above, and
nothing else moves.
