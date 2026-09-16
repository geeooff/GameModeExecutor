# Lot 2 — Configuration, and how commands run

**Goal.** Control over how several commands run for one event, and a
configuration that fails loudly and precisely when it is wrong.

**Done when:** a configuration with several commands per event runs them in
the configured order or concurrently; an invalid one makes the program refuse
to start, say exactly where the problem is, and exit with a dedicated code.
Closed 2026-09-10.

- [x] Several commands per event, at game start and at game stop
- [x] Validation errors that point at a line and column
- [x] Per-event execution mode: series or parallel
- [x] A dedicated exit code when the configuration is invalid
- [x] Every field documented in the shipped example
- [x] The stop-edge log line no longer claims a process id it cannot vouch for

**Verified 2026-09-10.** Two commands sleeping two seconds each: 4.54 s in
series, 2.22 s in parallel. Exit codes measured end to end: 3 for a missing
file, 4 for one that will not parse, 4 for one that fails validation, 0 for a
good one, 2 from the argument parser.

## Staying on TOML

A move to JSON was considered and dropped. The INI-like shape is the point,
and TOML keeps three things JSON would have cost: comments, no doubled
separators in Windows paths thanks to single-quoted literal strings, and parse
errors that are already better than anything hand-rolled:

```
TOML parse error at line 6, column 14
  |
6 | stop_delay = 5s
  |              ^^
string values must be quoted, expected literal string
```

A misspelt key, thanks to `deny_unknown_fields` on every table:

```
TOML parse error at line 2, column 1
  |
2 | log_levle = "info"
  | ^^^^^^^^^
unknown field `log_levle`, expected one of `stop_actions_on_exit`, `log_level`, `log_dir`
```

Line, column, a caret under the offending token, and the list of valid names.
Nothing to build.

What is still weak: semantic errors, checked after parsing, carry no position —
`detection.poll_interval must be greater than zero` says what but not where.
Spanned deserialisation could fix it; a configuration this small does not
obviously need it.

## Exit codes

`clap` already returns 2 for command-line misuse, so that value is spoken for.

| Code | Meaning |
| --- | --- |
| 0 | success |
| 2 | command line misuse |
| 3 | configuration file not found |
| 4 | configuration invalid: syntax or validation |
| 5 | another instance is already running |
| 1 | anything else |

## Series or parallel

Series existed in substance through the per-action `wait` flag; this lot made
it an explicit per-event mode. Parallel needed a defined answer for failures —
one command failing must not prevent the others, and the log has to say which
one failed — and for the stop actions racing the next game start, since
`stop_delay` no longer serialises them.
