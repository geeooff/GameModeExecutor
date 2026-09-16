# Lot 4 — Logs that speak to two readers

**Goal.** One log that serves two readers. Someone who wants to know what
happened reads it at `info` and sees plain sentences about games. A technician
reads the same log at `debug` and gets those lines annotated, plus the
reasoning behind them.

**Done when:** the same session reads correctly at `info` for a non-technical
reader and at `debug` for someone diagnosing, with no line written twice.
Closed 2026-09-10.

- [x] Categories replace Rust module paths: `watcher`, `game`, `commands`
- [x] The level filter is generated from the category list, never hand-listed
- [x] A test refuses any logging call without a category
- [x] One `FormatEvent` for both sinks: aligned category column, colour only on a real terminal
- [x] The message carries the sentence; structured fields carry the technical annex
- [x] Fields printed only when the reader asked for `debug` or `trace`
- [x] Every call site audited against the contract below

## Who each level is for

This is the contract, and it is a promise to whoever opens the log rather than
an internal convention.

| Level | Reader | Rule |
| --- | --- | --- |
| `error` | anyone | Something needs you. Name the file or command and what to check. No jargon. |
| `warn` | technician | A degradation the program absorbed. May be technical. |
| `info` | anyone | The story of a session, in plain sentences. |
| `debug` | technician | *Why* the program did what it did, plus every `info` line annotated. |
| `trace` | technician | Raw measurements. |

**`info` is reserved for what the program is for**: a game was detected, named
or lost, and the watcher started or stopped. Everything else had to earn its
place or move down. Three lines lost that argument — the writer path echoed at
startup, each command being started, each command's exit status — and are now
`debug`. That took `info` from 11 lines to 8, five of which are the detection
itself.

A command that exits non-zero is a **warning**, not a debug line — corrected
on 2026-09-16, after a `0xc0000142` that explained an evening's failure sat at
debug level while the fans raised the alarm.

`tracing` has five levels and `FATAL` is not one of them. A panic hook logs
`FATAL:` in the message at `error` level, with the build and the location,
rather than inventing a sixth level nobody's filter knows about. For it to
survive `panic = "abort"`, the log is written synchronously: nothing is
flushed after an abort, so a buffered crash report is one that never arrives,
and at a handful of lines per session the buffering bought nothing anyway.

## Why the messages are not centralised

Collecting every log string in one module was considered and declined. In Rust
that breaks locality — one jumps to another file to learn what a line says —
and turns literals the compiler checks into runtime `format!` calls. It is not
the idiom, and `tracing` is built against it.

What is centralised is the machinery: the categories, the filter built from
them, the single formatter, the error types. That is where duplication hurts.

The door is open. If the tone of the public lines drifts, the fourteen
`info`/`error` messages can move behind an enum with a `Display` impl — one
file, reviewable in a pass, unit-testable, and the only route to localising the
log, which is worth having eventually and nowhere near worth it now.

## The trap this lot walks past

An event whose target is not in `target::ALL` matches no filter directive and
is dropped **silently** — no warning, the line simply never appears. Two things
guard it: the filter is generated from the same list the categories come from,
so the two cannot drift, and `every_log_site_declares_a_category` fails the
build for any logging call without a `target:`. That test caught its own
author's first version.
