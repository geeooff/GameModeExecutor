# Lot 15 — Games Windows knows only from you

**Status: done 2026-09-23.** Proposed
2026-09-20 and taken ahead of everything else pending, on the finding
below; taken up once [Lot 9](09-robustness.md)'s configuration work had
closed its field run.

- [x] The instrument: a `watch-games` command in `presence-probe` that logs registry change notifications on Windows' game list and which entry's `LastAccessed` moved — built 2026-09-23 and checked on a scratch key, below
- [x] The measurements below, before any line of the watcher changes — 2026-09-23: Windows writes the entry at every launch, the registry notification never comes, the second signal adds about 25 us to a 4 ms poll, an elevated process grants `SYNCHRONIZE`; the untick mid-game is left to the field run
- [x] Detection from Windows' list as well as from the presence writer: a hand-marked title is a session from its launch — built 2026-09-23, three engine scenarios
- [x] A title marked *while it runs* becomes a session at the next idle look, and the start commands run then — built 2026-09-23: the list is read again when its key's last-write time moves
- [x] The idle cost measured and written down: no polling of the registry, and whatever polling of processes remains, with its figure — 2026-09-23, below
- [x] The idle poll made cheap: the process ids alone every `poll_interval`, names only for processes not seen before, a full snapshot every 30 s as the net for a reused id — built 2026-09-23
- [x] At start, a log line for each hand-made entry Microsoft's own list now covers, so the box can be unticked; `status` says the same — built 2026-09-23, checked on the real list: DS2, Wreckfest 2 and cs2 covered, *The Other Side* and a browser not
- [x] Verified in the field on the maintainer's machine — 2026-09-23, 13:36–14:26: *The Other Side* from launch and from a tick mid-game, Starfield through the writer unchanged, the idle processor measured against 0.2.0; DS2 and Wreckfest 2 needed no box any more, Microsoft's list having them

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

## The third run: nine ways of being told, 2026-09-23, 12:40–12:45

The maintainer's question after the second run: was the notification the
wrong one, or asked the wrong way? The first two runs had tried one way.
`presence-probe watch-methods` armed eight at once, each on its own event —
the predefined `HKCU` handle over the list's subtree as before; the same key
opened through `HKEY_USERS\<sid>`, and through `RegOpenCurrentUser`; the list
alone without its subtree; the parent `GameConfigStore` and all of
`HKCU\System` with every filter; each of the 141 entries watched on its own;
and the call made synchronously on a thread that blocks in it. A ninth ran
beside it, outside the probe: WMI's `RegistryTreeChangeEvent` on
`HKEY_USERS\<sid>\System\GameConfigStore\Children`, from an unelevated
PowerShell. Starfield launched at 12:42 and quit; *The Other Side* launched
at 12:44, unticked, ticked again, quit.

| Change | Found by reading | WMI | The eight `RegNotifyChangeKeyValue` ways |
| --- | --- | --- | --- |
| Starfield's `LastAccessed`, launch | 12:42:09.407 | three events, 12:42:09.257–.267 | none |
| *The Other Side*'s `LastAccessed`, launch | 12:44:26.120 | two, 12:44:25.978–.986 | none |
| its entry removed, the untick | 12:44:44.463 | one, 12:44:44.454 | only the entry's own handle, 12:44:44.452 — then `ERROR_KEY_DELETED` on re-arming it |
| a new entry, the tick | 12:44:49.464 | four, 12:44:49.411–.424 | none |

So Windows does report these writes, as they happen — to WMI's registry
provider, which runs in `WmiPrvSE.exe` under a system account — and does not
report them to an ordinary process asking the same question of the same
key, whichever way it asks, except that deleting a key still wakes whoever
watches that very key. Why is not established, and this record does not
guess further than that.

What WMI costs could only be seen from outside, the provider's process not
being open to an unelevated reader: over one idle minute with the
subscription, the busier of the two `WmiPrvSE` processes averaged 0.78 % of
a core, and 0.68 % over the next minute with the subscription gone — the
same, other programs on this machine use WMI too — and the only difference
the counters show is one burst of about twenty I/O operations in the minute
with it.

## Why the notification does not come: what the literature says

Decided 2026-09-23 by the maintainer: WMI is not taken — it adds a COM
client, a dependency on a service, and a fail-over between two sources of
wake-ups, for a behaviour nobody here can explain — until the reason the
direct notification stays silent is known. What was looked for, and found:

- **Microsoft's page for `RegNotifyChangeKeyValue`** names one kind of change
  the function cannot see: *"This function cannot be used to detect changes
  to the registry that result from using the RegRestoreKey function."* If
  whatever writes the game list does it by restoring keys rather than setting
  values, the silence follows. The same page says a second call on a handle
  with different parameters *"will succeed but the changes will be ignored"*
  — not the case here, every method had its own handle — and that repeated
  calls with the same parameters leak waits, which the probe no longer does.
- **Raymond Chen** (*The Old New Thing*, 2020-05-07): a deleted key ends
  its notifications for good, and a key recreated under the same name is a
  new key nobody is watching. That explains why the removed entry woke its
  own handle and nothing else; it does not explain Starfield, whose entry
  was not deleted and whose own handle still heard nothing when a value on
  it changed.
- **A reported case in the other direction**, `microsoft/WindowsAppSDK`
  issue 4075: a *packaged* WinUI 3 application watching `HKCU` receives no
  notification where the same code unpackaged does, while reading the value
  sees the change. Here the reader is unpackaged and the Game Bar, which
  writes, is packaged. Packaged applications see a registry merged from the
  real hive and per-application hives; whether their writes through that
  merged view reach watchers of the real hive is not documented.
- Nothing found names `GameConfigStore` with change notifications, on
  Microsoft's pages, Stack Overflow or elsewhere.

So two documented mechanisms fit the evidence — a restore, or a write
through a packaged application's merged registry — and neither is
confirmed. What would settle it is seeing the operation itself: Process
Monitor, filtered on `GameConfigStore`, shows which process writes and with
which call. It needs administrator rights to install its driver, on the
maintainer's machine and by the maintainer's hand; the watcher never will.

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

## The cost, and an elevated process, 2026-09-23

`presence-probe cost` times, over 300 rounds ten milliseconds apart, the
release build on this machine with 282 processes and 3 hand-made entries:

| Step | Median | 95th | Max |
| --- | --- | --- | --- |
| the process snapshot, which the idle poll takes today | 4181 us | 4609 us | 5334 us |
| the writer found in it, today | 3 us | 3 us | 6 us |
| the hand-made entries' names compared against it | 13 us | 13 us | 14 us |
| one full-path query, paid only for a name that matches | 55 us | 64 us | 167 us |
| the list key's last-write time | 11 us | 13 us | 17 us |

The second signal adds about 25 us to a poll that already costs about 4 ms
— the snapshot is the price, and it is paid today. Nothing new wakes the
watcher: the same poll, every `poll_interval`, two seconds by default.

Asked the same day whether the interval should grow to spend less: measured
first whether the snapshot is the only way to see new processes. It is not.
`K32EnumProcesses`, the process ids alone, took **32 us** median over 300
rounds (p95 38, max 77) against the snapshot's 4068 us, with 290 processes
running — about 130 times less. A poll that lists the ids, and asks only
the processes it has not seen before for their name (52 us each, a handful
a minute on an idle desktop), would cost about 50 us instead of 4 ms: at
two seconds, some 0.003 % of a core instead of 0.2 %. The interval then
stops being the lever, and the two seconds of reaction stay. One risk to
measure before relying on it: Windows reuses process ids, and a game that
takes the id of a process that exited since the last poll would look like
a process already seen.

And the fourth question: FanControl, which runs elevated here through its
scheduled task, granted both `SYNCHRONIZE` and
`PROCESS_QUERY_LIMITED_INFORMATION` to the unelevated shell — so an
elevated game can be waited on and its path read. A protected process, as
some anti-cheat runs, is not measured.

The fifth, the untick mid-game, is behaviour rather than a question: the
entry goes at once (12:04:07 above), so the session ends at the next poll
and the stop commands run. The field run checks it. **Changed when built,
2026-09-23:** a session on a game marked by hand parks on the game's
handle and looks at nothing while it runs, as a session on the writer
does; noticing an untick would have meant waking every poll interval for
the whole game. The untick takes effect at the game's next launch, and the
session ends when the game does.

## Process ids, from Microsoft's documentation first — 2026-09-23

The maintainer's rule, the same day: Microsoft's documentation first, a
spike only for what it leaves open. What it says:

- An id identifies a process *"until the process terminates"*, and *"after
  the process has terminated, the system can reuse the Id property value for
  an unrelated process"* (`System.Diagnostics.Process.Id`); `Win32_Process`
  says the same of the WMI class; *Process Handles and Identifiers*, that
  the identifier *"is valid from the time the process is created until the
  process has been terminated."*
- Raymond Chen gives the exact rule (*When does a process ID become
  available for reuse?*, 2011-01-07): the id belongs to the process object,
  which lives as long as the process runs *or anyone holds a handle to it*.
- Nothing documents how soon a freed id comes back.

So the spike measured that: 400 short `cmd.exe /c exit` processes started
one after another in 7.9 s — about fifty a second, far above an idle
desktop's churn — each handle closed as soon as it had exited. 345 distinct
ids; 55 came back, the soonest **2.8 s** after the id's previous process
started, the median 5.6 s, none within 2 s. Reuse is real and can be quick
under churn, and it is not instant.

What the poll does with that:

- **Every `poll_interval`, the ids alone** (32 us), compared with the last
  poll's; a process not seen before is asked its name (52 us), and its full
  path only when that name is the writer's or a hand-made entry's.
- **Every thirty seconds, the full snapshot** it takes today (4 ms), which
  names every process afresh. It is the net for an id reused between two
  polls: a game that took a freed id is still found, within thirty seconds
  instead of two. At an idle desktop's churn that case needs a process to
  exit and a game to start under its id within one poll interval; the spike
  never saw it happen within two seconds even at fifty processes a second.
- **Holding a handle to every process** would close the gap by the rule
  above, and was set aside: a few hundred handles in the watcher, to every
  process including games, is a footprint players and anti-cheat both look
  at, for a case the net already covers.

The cost: about 50 us a poll instead of 4 ms, and the snapshot's 4 ms every
thirty seconds — some 0.016 % of a core at two seconds, against 0.2 %
today. The maintainer's own `poll_interval`, raised to five seconds to
spend less, goes back to two once this is in.

## Microsoft's list itself, and the hint at start — 2026-09-23

The maintainer asked for one more thing: say, at start, which hand-made
entries Microsoft's list now covers, so the box can be unticked and Windows
recognise the game by itself. That needs Microsoft's list, which is not the
registry's `Children` key — that holds the entries Windows *created* from
it. The list is a file, `%LOCALAPPDATA%\Microsoft\GameDVR\KnownGameList.bin`,
1.8 MB here, dated 2026-08-18; its header carries 2691 at offset 8, the
`KGLRevision` the registry reports. Its format is not documented.

What reading it showed:

- A search for a name is wrong: `DS2.exe` is the end of `borderlands2.exe`,
  `chrome.exe` the end of *Blazing Chrome*'s `blazing chrome.exe`.
- The executable's name is a field of its own, UTF-16 and preceded by its
  length in bytes as 16 bits; the title's folder names, a GUID and the Xbox
  `TitleId` follow. Matched as whole fields, the answers are exactly right:
  `DS2.exe` once, with `DEATH STRANDING 2 - ON THE BEACH`; `Wreckfest2.exe`
  once, with `wreckfest 2`; `cs2.exe` once, with `Counter-Strike Global
  Offensive` and `win64`; `chrome.exe`, *The Other Side*, Fallout and 3DMark
  not at all.
- The GUID each record carries is the `GameDVR_GameGUID` of the entry
  Windows created from it: `f5ec2e1c-…` for DS2, `24762c8e-…` for Wreckfest
  2, in the file and in the registry alike. The file is the source.

So at start, for each hand-made entry, the watcher looks for a record whose
executable field is that entry's file name and one of whose folder names is
a folder of that entry's path — the two things Windows' own record says it
matches on — and says, at `info`: *DS2.exe is marked as a game by hand, and
Microsoft's list knows it now: untick "Remember this is a game" in the Game
Bar, and Windows will recognise it by itself.* Once per start, never again
until the next. Because the format is undocumented, a file that cannot be
read or does not parse the way described here produces no hint and one
`debug` line saying why — the hint is advice, and wrong advice is worse
than none.

## The field run, and what the watcher costs — 2026-09-23

The maintainer, with the lot's build installed at 13:32: `poll_interval`
back to two seconds in their own file at 13:36 — reloaded live —, then
*The Other Side*, marked by hand: detected at 13:59:39 (*Game detected:
TheOtherSide-Win64-Shipping.exe, which is marked as a game by hand in the
Game Bar*), ended 14:00:21, the stop commands two seconds later, the fans
following both times. Launched again and unticked mid-game at 14:01: the
session went on until the game quit, as decided when built, and the list
read afterwards had two entries. Launched unticked at 14:02 and ticked
mid-game: the list was read again and the session started in the same
look, 14:02:36.629. Starfield at 14:04, through the writer, unchanged.

The installed watcher at idle, measured over nine minutes each, same
machine, same two-second interval, same minute:

| | 0.2.0 as published | this lot |
| --- | --- | --- |
| processor, idle | 1750 ms in 540 s — **0.324 %** of a core | 140.6 ms in 540 s — **0.026 %** |
| private memory, just started | 1.9 MB | 2.0 MB |
| handles, just started | 157 | 163 |

Twelve and a half times less processor at idle, measured on the whole
process rather than estimated from its steps. The first measurement of
this lot's build read 5.1 MB and 424 handles — but it had played four
sessions since it started and 0.2.0 none. `presence-probe footprint` runs
the watcher's steps one at a time and reports what each leaves: this lot's
look, its hand-made entries and its reading of Microsoft's list together
leave 0.24 MB and two handles, and the same after a hundred looks — no
leak. Naming a game (every process asked its path and package) leaves
0.13 MB. **Reading the GPU counters for the refinement — there since
[Lot 3](03-game-naming.md) — leaves 3.7 MB** the first time, and no more
after: a cost paid once, at the first session, not a leak. It accounts
for about twenty handles in the probe; the watcher gains some 260 over its
first sessions, and the rest is not yet traced. Both are proposed as a
separate piece of work, [Lot 16](16-footprint.md) — the lot's own cost is
nil.

## What it changed in the program — built 2026-09-23

- **`sensor::Sighting`**: the idle look answers *the writer*, or *a game
  marked by hand* with its exact name, path and process. The engine parks
  on either's handle; a game marked by hand is not refined, since its entry
  names it exactly; the grace after an exit accepts either coming back. The
  marker, the resume, the handover and the reload are untouched — a session
  is a session. Three scenarios in `engine/tests.rs`: a session from launch
  with both edges and no rename, a resume after a handover, a relaunch
  within the grace.
- **`detect::hand_made`**: the entries with `Revision = 1`, no `TitleId`
  and a path; the list's key kept open to ask its last-write time each look.
- **`detect::process::Tracker`**: the ids every look, a name for a new id
  only, the full snapshot every thirty seconds.
- **`detect::microsoft_list`**: `KnownGameList.bin` read for whole-field
  matches only, as described above; at start, one `info` line per hand-made
  entry it covers. A first version also required each folder name's length
  to be repeated after it, which Counter-Strike's record seemed to show; on
  the real file DS2 came out *not listed*, its record having `01 00` there.
  The rule is now two characters or more for a folder name, which keeps the
  one-character artefact out, and DS2's record is a test byte for byte.
- **`status`** lists the games marked by hand, each with whether it runs and
  whether Microsoft's list knows it; **`presence-probe microsoft-list`** asks
  the list about any path.

**The review before the pull request, 2026-09-23**, found and fixed: a
hand-made sighting carried its process id as an `Option` that a missing
value would have turned into 0, a wait that ends at once — now a field;
the list's key, when missing at start on a profile the Game Bar had not
written yet, was never tried again; Microsoft's list was decoded twice for
every entry, compared names in ASCII only and looked 600 bytes past a name,
less than two long folder names — now decoded once, compared in lower case
beyond ASCII, and looked at over three folder names' worth; the writer's
path was lowercased again at every look. And two rules sat inside system
calls: which process is a sighting, and which entries Microsoft's list
covers. Both are now pure functions the tests drive with made-up processes
and records — `sensor.rs` went from 8 % of its lines covered to 43 % on a
hosted runner, `hand_made.rs` from 37 % to 48 %; on a client machine, with
the tests that read it, the new modules are covered at 81 to 98 % and the
library at 72.8 %, up from 70 % at Lot 13.
