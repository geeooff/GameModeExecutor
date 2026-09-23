# Lot 16 — What the watcher keeps

**Status: in progress since 2026-09-23.** Proposed the same day in
[Lot 15](15-marked-games.md)'s record, which found that reading the GPU
counters left 3.7 MB behind and that the installed watcher had gained some
260 handles over its first sessions, only twenty of them traced; taken up
at once, at the maintainer's request.

- [x] The instrument: `presence-probe footprint` step by step, `menu-cost` for what one click leaves over five minutes, `gpu-load` to set the GPU reader beside Windows' own `typeperf`, and a scratch watcher driven through sessions and reloads — 2026-09-23
- [x] Where the handles come from — 2026-09-23, below: the GPU read, the shell and the update check; sessions and reloads leave nothing
- [x] The GPU counters read through PerfLib instead of PDH — built 2026-09-23: 0.23 MB and 18 handles left instead of 3.6–3.9 MB and 19, the values checked against `typeperf`
- [ ] The menu's shell opens: a proposal below, the maintainer's call
- [ ] What *Check for updates* leaves: a proposal below, the maintainer's call
- [ ] Measured on the whole installed process against 0.3.0: just started, after a session whose refinement reads the counters, after the menu's clicks
- [ ] What the watcher costs, written for the people who use it in [How it works](../how-it-works.md), from those figures
- [ ] Verified in the field: a session whose refinement reads the counters, on the maintainer's machine

**Done when** a session, a reload and each entry of the menu leave the
watcher holding no more than a figure written here over what it held just
started, measured on the whole installed process against 0.3.0 — and every
figure the documentation gives for what the watcher costs is one measured.

## Sessions and reloads leave nothing, 2026-09-23

A watcher built from this branch before any change (0.3.0, `da26e0f2`), on
a scratch configuration — two-second poll, one-second stop delay, the
refinement three seconds in, three `cmd /c exit 0` commands — with the
installed one stopped. Sessions made by activating Windows' presence
writer, as `presence-probe activate` does; reloads by rewriting the file
with another `log_level`. Three runs:

| | handles | private | threads |
| --- | --- | --- | --- |
| just started | 168–170 | 2.1–2.2 MB | 6 |
| after the first session | 174 | 2.3 MB | 6, then 3 |
| after the other sessions, up to five, and the reloads | 174 | 2.3–2.4 MB | 3 |

The three threads that go are the loader's own workers, which Windows ends
when they have been idle. The first run read differently — 225 handles
after its third session and 371, with 12 threads, after its first reload —
and two runs after it, the second sampling four times a second, never
did it again. Its size is the size of a shell call, measured below, and
nothing in that run is known to have made one: recorded as not reproduced,
not explained.

None of these sessions read the GPU counters. The refinement reads them
only when two processes or more match the game ([Lot 3](03-game-naming.md));
a presence writer started by hand has no game, and the log said
*Refinement has nothing to arbitrate* every time.

## What each step leaves

`presence-probe footprint` runs the watcher's steps one at a time in one
process and reports its private bytes and handles after each. A step that
leaves the same after ten more is a cost paid once; one that leaves more is
a leak. None leaks. The steps that cost anything:

| Step, the first time | private | handles |
| --- | --- | --- |
| naming a game: every process asked its path and package | +0.1 to +0.2 MB | +2 |
| reading the GPU counters through PDH, 0.3.0 | **+3.6 to +3.9 MB** | +19 |
| reading them through PerfLib, this lot | +0.23 MB | +18 |
| opening a program through the shell, as the menu opens a file | **+1.45 MB** | **+152** |

`presence-probe menu-cost` makes one click's worth of work in a fresh
process and samples it for five minutes. The shell is asked to run a hidden
`cmd /c exit 0`, which takes the same road as *Edit configuration* without
putting anything on screen; *Check for updates* goes over the network as
the menu's does:

| One click | 1 s after | 30 s | 120 s | 300 s |
| --- | --- | --- | --- | --- |
| a shell open, `ShellExecuteW` | +1.48 MB, +167 handles, +6 threads | +1.35 MB, +150, +5 | +1.23 MB, +147, +1 | **+1.20 MB, +141, +0** |
| *Check for updates*, WinHTTP | +1.48 MB, +162, +6 | +1.43 MB, +156, +6 | +1.08 MB, +125, +2 | **+1.03 MB, +111, +1** |
| a plain `CreateProcess` | +0.01 MB, +2, +0 | | | |

The threads are the thread pool's and expire within five minutes; the
handles and the memory stay for the life of the process. Initialising COM
first, as Microsoft asks callers of `ShellExecute` to do, changed nothing
— 314 handles and 10 threads either way — and neither did making the call
on a thread that ends afterwards (312 and 10). The cost belongs to the
shell, not to how it is called.

What this says of the watcher that read 424 handles after four sessions in
Lot 15 is inferred, not measured: one shell open would account for some 150
of its 260 — the maintainer changed the poll interval in their file at
13:36 that day, by a road not recorded — and a naming or two for a few
more. The rest is not traced to a step; the whole-process measurement
below is where it will be looked for again.

## The GPU counters: PDH keeps what it loaded

What Microsoft documents: `PdhCloseQuery` "frees all memory associated with
the query" — the query's own. What PDH loads to resolve a counter path is
the process's, and nothing in the documentation releases it. Measured in a
fresh PowerShell process: `pdh.dll` is the only module added, adding the
counter is the step that leaves the memory and seven handles, and closing
the query gives two handles back.

*GPU Engine* is a V2 counter set: registered by the display kernel,
`dxgmms2.sys`, provider *GPU Performance Counters*, two counters, *Running
Time* and *Utilization Percentage*. For V2 counter sets Microsoft documents
a second consumer, the PerfLib functions, for collecting "with minimal
dependencies and overhead". Its guide recommends PDH for most applications
and says these are harder to use; what is harder is a documented byte
layout to walk, which is written once and tested without Windows.
<https://learn.microsoft.com/en-us/windows/win32/perfctrs/using-the-perflib-functions-to-consume-counter-data>

How the reader does it, each step as the guide describes:

- The counter set is found by its English name among those
  `PerfEnumerateCounterSet` lists — 162 here, 33 ms the first time — and
  the counter by its English name in the set, since hard-coding their
  identifiers needs the provider's symbol file, which is not published.
- The counter's registered type is checked: `0x20510500`,
  `PERF_100NSEC_TIMER`, whose formula is `100 × (N1 − N0) / (D1 − D0)` with
  `D` the sample's time in 100 ns units
  ([Calculating Counter Values](https://learn.microsoft.com/en-us/windows/win32/perfctrs/calculating-counter-values)).
  Another type is refused, not guessed at: no opinion, as when the counters
  cannot be read at all.
- One query, every instance (`*`), two samples a second apart; an instance
  missing from one of them, or whose value went backwards, says nothing —
  Microsoft's own example drops those samples too.
- `PERF_DATA_HEADER`, `PERF_COUNTER_HEADER`, `PERF_MULTI_INSTANCES`,
  `PERF_INSTANCE_HEADER` and `PERF_COUNTER_DATA` are read by offset with
  every size bounded by the block around it; a cut or inconsistent block is
  an error, and ten tests build such blocks by hand.

Set beside `typeperf "\GPU Engine(*engtype_3D)\Utilization Percentage"`
over the same seconds, 17:10:34–40, the desktop idle: the same three
processes at the same magnitudes — 2.39, 1.65 and 1.08 % against 2.87,
1.71 and 1.09 % for the first second — and in the same order in three of
the four seconds compared, the two windows half a second apart. A game's
load, tens of percent on one process, is the field run's to confirm.

## Proposed, for the maintainer's decision

**The menu's shell opens, through a short-lived helper.** *Edit
configuration*, *Open log folder*, *Documentation* and the release page
all go through `ShellExecuteW`, and the first of them leaves 141 handles
and 1.2 MB for the rest of the watcher's life. The watcher could start its
own executable with a hidden command that makes that one call — and the
Notepad fallback when a `.toml` has no association — and exits: a plain
`CreateProcess` leaves two handles. What it costs: one hidden command, the
open's outcome read from the helper's exit code, some 30 ms on a click.
Recommended: it is the larger of the two costs a player can see in Task
Manager, and the code it takes is small and ordinary.

**What *Check for updates* leaves, left as it is.** 111 handles and 1 MB
after the threads expire. The check is a click made once a release; a
successful update replaces the watcher anyway; moving the check into a
helper would mean handing its verdict back across processes — a second
protocol, for a figure the helper saves once a month. Written down in
[How it works](../how-it-works.md) instead, with the other figures.
