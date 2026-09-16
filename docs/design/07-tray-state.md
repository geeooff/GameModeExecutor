# Lot 7 — Icon, tooltip and menu as one state

**Goal.** The icon says at a glance whether a game is detected, and which one.

**Three surfaces, one truth.** The icon, the tooltip and the menu all answer
the same question, so they are one piece of work and not three. Any of them
left behind is worse than none: an icon gone green under a tooltip still
saying "no game detected" is a program contradicting itself.

**Done when:** starting and stopping a game moves all three together, and a
title Windows tracks but does not name reads sensibly everywhere rather than
showing an empty space. Closed 2026-09-15 against Starfield, confirmed on
Skyrim and Battlefield 6 the same night.

- [x] The engine tells the tray when a game starts, is renamed by the refinement, and stops
- [x] **Icon** switches between idle and active, both ways
- [x] **Tooltip** names the running game, or says one is running when it has no name, or says none is
- [x] **Menu** gains a disabled first entry saying the same thing
- [x] One wording for "running but not named", shared by all three
- [x] The tooltip's 127-character limit respected, with an ellipsis rather than Windows' silent cut

## How they are kept from drifting

There is **one** `Session` value; the icon's state, the tooltip and the menu
header are all *derived* from it when needed rather than stored alongside it.
The tray keeps one piece of remembered state, `shown` — the state, theme and
tooltip the shell is displaying right now — so a reload can tell at a glance
whether there is anything to do.

The engine does not know the tray exists. It takes an optional callback — a
game started, was renamed, ended — and the caller decides what that means. The
engine stays the part worth keeping testable.

`fire_stop` reports **before** running the stop commands rather than after:
those can take fifteen seconds, and an icon still showing a game that ended
that long ago is precisely what someone would notice.

## The sessions that closed it

`presence-probe activate` was the hope for testing without a game: activating
the runtime class ought to make Windows start a presence writer. It does not —
the activation resolves in-process — so the last mile waited for a real
session.

```
16:15:54.699  Game detected: Starfield.exe   matched_by="package family"
16:15:54.699  icon refreshed  state=Active   tooltip=... playing Starfield.exe
16:16:14.831  Refinement has nothing to arbitrate, keeping the current name  candidates=1
16:16:27.385  Windows released the presence writer; the game had already exited
16:16:29.394  Game no longer detected: Starfield.exe
16:16:29.395  icon refreshed  state=Idle     tooltip=... no game detected
16:16:29.500  FanControl - Quiet profile finished
```

The icon and the detection share a millisecond in both directions. **The stop
ordering is visible**: the icon went grey at `.395` and the FanControl command
finished at `.500` — 105 ms here, up to fifteen seconds whenever `schtasks` is
slow. And the refinement declined and *said so*: that line exists because of
[Lot 4](04-logging.md), where a silent refinement was indistinguishable from
one that never ran.

Skyrim, a Steam install named through its executable path rather than a
package family, tightened two numbers: on the stop edge the icon and the
detection share the **same millisecond**, and on the start edge the icon
trails by 53 ms, which is the message crossing threads. The 2 s between
Windows releasing the writer and the session being declared over is the
configured `stop_delay`, not latency.

The rename reaching the screen — the tooltip changing under a running game —
was first seen on Battlefield 6; see [Lot 3](03-game-naming.md). Fixing the
name mid-session also fixes the last line of the session: `fire_stop` uses the
refined signal, so `Game no longer detected` names the game rather than the
launcher that happened to start first.

## What it settled about the window

A **top-level window that is never shown does receive broadcasts** — the theme
switch reached it, and so did `WM_QUERYENDSESSION` later. A message-only window
would have seen neither, and both would have failed silently. That vindicates
the choice made in [Lot 5](05-windowless-watcher.md).
