# Lot 15 — Games Windows knows only from you

**Status: proposed 2026-09-20, taken next, ahead of everything else
pending** — the maintainer's decision the same morning, on the finding
below. It waits only for [Lot 9](09-robustness.md)'s configuration work to
close its field run.

- [x] The instrument: a `watch-games` command in `presence-probe` that logs registry change notifications on Windows' game list and which entry's `LastAccessed` moved — built 2026-09-23 and checked on a scratch key, below
- [ ] The measurements below, before any line of the watcher changes — the first two answered 2026-09-23: Windows writes the entry at every launch, and the registry notification never comes; three to go
- [ ] Detection from Windows' list as well as from the presence writer: a hand-marked title is a session from its launch
- [ ] A title marked *while it runs* becomes a session within the settle time, and the start commands run then
- [ ] The idle cost measured and written down: no polling of the registry, and whatever polling of processes remains, with its figure
- [ ] Verified in the field on DS2 and the other hand-marked titles on the maintainer's machine

**Done when** a game the Game Bar knows only because the person ticked
*Remember this is a game* is detected at its launch, or at the tick if that
comes mid-game, with the same start and stop commands as any other title —
and the watcher still does nothing measurable while no game runs.

## What was found, 2026-09-20

The maintainer started *Death Stranding 2: On the Beach* from Steam at
11:33:57 and the watcher, running since 10:09, never said a word. The
machine, read at 11:38–11:42 while the game ran:

- `DS2.exe` up, `GameBar.exe` up from 11:34:07 — the maintainer had opened
  the overlay to check the toggle. The toggle was on; it had been ticked
  days before, so the list already knew the game when it was launched.
- Windows had *seen* the game: `GameDVR\LastGameActivity` read 11:34:00,
  three seconds after the launch, the same relation the record measured for
  Starfield and Farming Simulator in [Detection](00-detection.md); the game
  list entry's own `LastAccessed` carried the same second; `bcastdvr` had
  rebuilt its encoder topology at 11:34:00.990.
- **`GameBarPresenceWriter.exe` never started.** Not at 11:38, not after a
  minute's watch at 11:39. And the maintainer's own Xbox profile, in the
  screenshot they sent, read *Online* — not *Playing DEATH STRANDING 2* —
  while a friend's read *Palworld*.
- The entry itself, against the entries of titles the watcher has
  detected:

  | | DS2 | cs2, Skyrim, bf6, RDR2 |
  | --- | --- | --- |
  | `Revision` | 1 | 2691, the revision of the list Microsoft distributes |
  | `TitleId` | none | an Xbox title id |
  | `GameDVR_GameGUID` | none | a guid |
  | `Flags`, `Type` | 17, 1 | 17 or 19, 1 |

So the presence writer — *presence*, as in the Xbox status "playing such
and such" — is activated for titles Windows knows by their Xbox identity,
and a title the person taught it by hand has none: a game to the Game Bar,
to GameDVR, to Game Mode, and nothing for the writer to write. Inferred from
the three facts above, not read in any documentation; the figures are the
record. It also explains the nephew's machine on 2026-09-18: `chrome.exe`
was a hand-made entry there, the writer had been started by Overwatch, and
Chrome only supplied the first name. (Corrected 2026-09-23: that Chrome's
entry there is hand-made is inferred — nobody has read that registry — and
the first run below amends the rest of this paragraph.)

The detection page promised that "anything Windows treats as a game"
triggers the watcher. That was true of every title measured before this
one, and it is false for this class; corrected in place there and in the
user pages the same day.

## What is decided

- **The signal stays Windows' verdict, and gains Windows' list.** The
  program already reads `HKCU\System\GameConfigStore\Children` to *name*
  the game; it is Windows' own list, grown by Microsoft's revisions and by
  the person's own ticks. A process on that list running is Windows saying
  a game is running, as much as the writer is. No list of our own, no
  fullscreen or GPU heuristics: a title neither Windows nor the person has
  named a game stays invisible, and that is right.
- **Marking mid-game counts.** The person starts a title Windows does not
  know, the fans stay on idle, they open the Game Bar and tick the box: the
  session starts *then*, as soon as the change reaches the watcher, and the
  start commands run. Nothing to relaunch.
- **No polling of the registry.** `RegNotifyChangeKeyValue` on the list's
  key, subtree, for names and values, gives a kernel event the watcher can
  park on beside the stop event — documented, unelevated for `HKCU`, and the
  same shape as the writer's handle. The Game Bar writes several values into
  a new entry, so the wake-up settles for a moment before the list is read
  again, as the configuration reload does with its folder. **Contradicted
  by measurement on 2026-09-23**: the notification never comes for the Game
  Bar's writes — see the second run below. What replaces it is in *What
  the runs decide*.
- **The cost while idle is the budget.** Today it is one process lookup
  every `poll_interval`. Whatever the design below adds while no game runs
  is measured and written here before it is accepted.

## To measure first, in this order

1. **Does the list's `LastAccessed` move at every launch, for every kind of
   entry?** It did for DS2 today (a hand-made entry) and it agrees with
   `LastGameActivity` for listed titles. If Windows touches the entry of
   *every* game it detects at launch, then a registry notification on the
   subtree is a wake-up for *every* game start, listed or hand-made — and
   the idle poll for the writer could go with it, leaving the watcher parked
   on two kernel objects and nothing else. That would be the best outcome
   and it is the first thing the probe looks at.
2. **What `RegNotifyChangeKeyValue` delivers**: one-shot, so re-armed after
   each wake; whether a new subkey (the tick) and a value set on an existing
   one (the launch) both fire with `REG_NOTIFY_CHANGE_NAME |
   REG_NOTIFY_CHANGE_LAST_SET`; how many wake-ups one tick produces, for
   the settle; `REG_NOTIFY_THREAD_AGNOSTIC`, so the event can be waited on
   from the engine's thread rather than the one that armed it.
3. **Which process.** The entry names the executable's full path; the
   running process is found the way naming finds it today, from a Toolhelp
   snapshot matched against the list. Measure what a snapshot costs on this
   machine — it is what an idle poll would pay if step 1 disappoints.
4. **Waiting on the game's own handle.** `OpenProcess(SYNCHRONIZE)` on the
   matched process, so the session ends when it exits with nothing polled
   meanwhile — the writer's shape again. Measure whether an elevated game
   (a launcher that asks for administrator rights) grants `SYNCHRONIZE` to
   an unelevated watcher; if not, that title is named in the log as one
   the watcher can only poll.
5. **The end of a session** when both signals exist: the writer's grace
   (`stop_delay`) is for the writer's blinking; a process exit is an exit.
   And the odd case: the person unticks the box mid-game. The entry goes,
   Windows no longer calls it a game; the session ends and the stop
   commands run, which is what the person asked for. To confirm on the
   probe.

## The instrument, checked 2026-09-23

`presence-probe watch-games [secs] [key]` parks on `RegNotifyChangeKeyValue`
over the list's key — subtree, names and values, `THREAD_AGNOSTIC`, armed
once and re-armed after each wake — and on each wake reads the list again
and says what moved:
an entry added or removed, an entry whose `LastAccessed` moved (with how long
ago that time is), an entry changed otherwise, or nothing it compares. Each
entry is labelled *hand-made* (`Revision = 1`, no `TitleId`) or *listed*. It
also looks at the presence writer every 250 ms, so the two signals read side
by side in one log. The optional `key` is there to check the probe itself on
a key one can write to.

Checked that way on a scratch key under `HKCU\Software`, deleted after: a
`LastAccessed` set on an entry, an entry created with two values, a value
changed, an entry removed — six wake-ups, each said as it happened, within a
few milliseconds of the write. **An entry created and then given its values
is two notifications, not one**: the first saw the entry already complete,
the second found nothing new. The settle the watcher will need is measured
on the Game Bar's own writes next, not on these.

## The first run, 2026-09-23, 11:20–11:40

The maintainer played while the probe watched, and noted the times:
Starfield launched and quit; Wreckfest 2 launched and quit; DS2 launched,
its *Remember this is a game* unticked, relaunched; Wreckfest 2 the same;
*The Other Side* launched, unticked, relaunched, ticked again, quit. What
the probe and the registry said:

| Time | Game | Presence writer | The game's entry afterwards |
| --- | --- | --- | --- |
| 11:22:44 | Starfield (packaged) | started 11:22:44.844, exited 11:25:52 | `Revision 2`, no `TitleId`, `LastAccessed` 11:22:44, key written 11:22:44.836 |
| 11:26 | Wreckfest 2, ticked by hand | never | — (the ticked entry, removed at 11:34) |
| 11:29 | DS2, ticked by hand | never | — (the ticked entry, removed at 11:30) |
| 11:31:10 | DS2, after the untick | started 11:31:10.129, exited 11:32:32 | **a new entry**: `Revision 2691`, `TitleId 1653303105`, a `GameDVR_GameGUID`; key written 11:31:10.082 |
| 11:35:21 | Wreckfest 2, after the untick | started 11:35:21.756, exited 11:36:31 | **a new entry**: `Revision 2691`, `TitleId 2086335033`; key written 11:35:21.712 |
| 11:39:48 | *The Other Side*, ticked again mid-game | never | `Revision 1`, no `TitleId`, key written 11:39:48.103 |

Four things, the first of which changes this lot:

- **A hand-made entry shadows the one Microsoft distributes.** DS2 and
  Wreckfest 2 were in Microsoft's list all along — the entries Windows
  created for them the moment the hand-made ones were gone carry
  `Revision 2691`, and `KGLRevision` read 2691 on 2026-09-20 as it does
  today, so the list Windows held when DS2 went unseen already knew it.
  While the person's own entry matched, Windows used it — no `TitleId`, no
  writer, *Online* in the Xbox overlay. Unticked, the next launch matched
  the distributed list, the writer started, the overlay said *Playing*.
  The maintainer's reading — Xbox added the game after its release, after
  the box had been ticked — fits every figure; when Microsoft's list
  gained it is not recorded anywhere this machine can see. *The Other
  Side* is the control: not in the list, unticked and relaunched it stayed
  unknown, and ticked again it got a `Revision 1` entry and no writer.
- **So there are two classes, not one.** Titles ticked before Microsoft
  listed them: a remedy exists today — untick, relaunch — and the program
  can say which entries are candidates, since a hand-made entry for an
  executable the distributed list also names is exactly that. Titles
  Microsoft does not list at all: only the rest of this lot reaches them.
- **Windows writes the entry at every launch, listed or not**, in the
  same quarter-second as the writer: the key 8, 47 and 44 ms before the
  probe's look found the writer, which looks every 250 ms, so the order
  within that quarter-second is not measured. `LastAccessed` moved for
  Starfield, a listed packaged title, as it had for the hand-made DS2 on
  the 20th.
- **The registry notification did not come.** Not once in twenty minutes,
  through the writes the keys' own last-write times confirm — 11:22:44,
  11:31:10, 11:35:21, 11:39:48 — and the removals the unticks made.
  The shell the probe ran from is not packaged — `GetCurrentPackageFullName`
  said so — so no container stood between it and the hive. The probe
  re-armed every 250 ms, which Microsoft documents as piling up waits; on
  a scratch key 480 re-arms later a write still woke it in 3 ms, so that is
  not shown to be the cause either. The probe now arms once per wake, and
  reads the list every 250 ms regardless, to log any change that arrives
  *without* a notification as `MISSED`. Its second run started at 11:48.
  Until it answers, the notification is not a design this lot can lean on.

## The second run, 2026-09-23, 11:48–12:05

The probe armed once per wake this time, and read the list every 250 ms
whatever happened, logging a change found without a notification as
`MISSED`. The maintainer launched Starfield, then *The Other Side* three
times — the first stuck on a black loading screen, killed — and on the
third unticked and ticked the box mid-game.

| Time | What happened | The probe |
| --- | --- | --- |
| 11:58:42.151 | Starfield launched | writer started; **MISSED** `LastAccessed` moved, 6 ms later in the same look |
| 12:00:14 | Starfield quit | writer exited |
| 12:00:47, 12:02:31, 12:03:45 | *The Other Side* launched three times | **MISSED** `LastAccessed` moved, each time; no writer, ever |
| 12:04:07 | the box unticked, game running | **MISSED** REMOVED, the hand-made entry `1782914d` |
| 12:04:31 | the box ticked again, game running | **MISSED** ADDED, a new hand-made entry `e2467066` |

Seven writes, seven changes found by reading, **no notification at all**.
The first run's silence was not the re-arming: the notification is not
delivered for these writes, to this unpackaged, unelevated reader, while
the same code on a key under `HKCU\Software` wakes within milliseconds. Why
is not established — the Game Bar is a packaged application and its writes
may reach the hive through a layer the notification does not watch — and it
does not need to be: it is measured twice, and a design that waits on it
waits forever.

What the run confirms besides:

- **Every launch writes the entry**, listed or hand-made, a launch that
  hangs on its loading screen included.
- **A tick mid-game creates an entry at once**, with a new key name; an
  untick removes it. One tick is one change for a reader that looks every
  250 ms; the Game Bar writes the entry's values close enough together that
  no look saw it half-made.
- **The hand-made mark is `Revision = 1`** on every entry known to have
  been ticked by hand — DS2, Wreckfest 2, *The Other Side* twice — and on
  the two other entries of that shape here, Fallout and 3DMark. Listed
  entries carry the distributed revision, 2691 here, or 2 for packaged
  titles.

## What the runs decide

- **No notification, and no new polling either.** The watcher already takes
  a process snapshot every `poll_interval` while no game runs, to look for
  the presence writer. The second signal reads the same snapshot: a running
  process whose full path is the `MatchedExeFullPath` of a hand-made entry
  is a session. Names are compared first, from the snapshot, and only a
  name that matches is asked for its full path, so the cost of a poll is a
  few string comparisons more than today.
- **The list is read again only when it changed**: one `RegQueryInfoKey` on
  the list's key per poll, for its last-write time, which a tick or an
  untick moves — measured on 2026-09-23, 11:39:48.102 for the key and
  11:39:48.103 for the entry the tick created. A launch that only moves a
  `LastAccessed` does not move it, and does not need to: the executable is
  on the list already. So a tick mid-game becomes a session at the next
  poll — two seconds by default — and no sooner, which is the price of not
  polling harder.
- **Hand-made entries only, by exact path.** Listed titles are the writer's;
  a hand-made entry shadowing a listed one is matched like any other. The
  parent-directory and package rules that *naming* uses stay out of
  detection: they match too loosely to decide that a session exists.
- **A title ticked by mistake becomes a session whenever it runs** — a
  browser, say. That is what Windows was told, and the Game Bar's own box
  is how to take it back. `status` will list the hand-made entries the
  watcher follows, and the log will name the entry a session started
  from, so the person can find which box to untick.

## What it changes in the program, once measured

Still to measure before building: what a snapshot and the path lookups
cost on this machine (the third item above), whether an elevated game
grants `SYNCHRONIZE` to the watcher (the fourth), and the end of a session
when the box is unticked mid-game (the fifth — the entry goes at once, so
the session can end at the next poll).

The engine's sensor gains a second question — which listed processes are
running, and a handle to wait on for one of them — and its loop treats
"the writer runs" and "a listed process runs" as one session with two
possible anchors. The refinement, the marker, the handover and the reload
are untouched: a session is a session. `status` says which signal it sees.
The user pages say plainly what *Remember this is a game* does for this
program, once it does something.
