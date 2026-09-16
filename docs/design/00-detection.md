# Detection: why the Game Bar presence writer

The foundation every lot stands on. Decided before Lot 1, on measurement.

## The signal

Windows ships a Game Bar *presence writer*, `GameBarPresenceWriter.exe`: an
out-of-process WinRT server it activates when it decides a game is present,
and releases when the game is gone. The watcher does not replace it or talk to
it. It observes whether that process is alive:

- **idle** — look for the writer every `poll_interval` (2 s by default);
- **playing** — park on the writer's process handle and do nothing at all
  until Windows releases it.

So the only cost while a game runs is one thread asleep in the kernel, and
there is no list of games to maintain: detection is Windows' own verdict.

Which executable to watch is read from the registry at run time, so a machine
where another tool owns the registration is still followed correctly, and a
rename by Windows servicing does not silently break detection. Matching is on
the full image path, not the file name.

## Why that signal, and not the others

The goal was to reuse what Windows already knows instead of maintaining an
allow-list of executables. Everything below was checked on Windows 11 25H2.

| Candidate | Verdict |
| --- | --- |
| **Game Mode** (`expandedresources.h`, `HasExpandedResources`) | Unusable. [Deprecated since Windows 10 1809](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/gamemode/game-mode-portal), and only ever callable from inside the game process. No supported way to ask whether Game Mode is active. |
| **Xbox Mode** (the Xbox full screen experience, internally "Gaming Posture") | Unusable. A shell home-app mode, not a detector, and its APIs are `Windows.Internal.*`. |
| **`Windows.Gaming.Preview.GamesEnumeration.GameList`** | Unusable. Requires the restricted `gameList` capability: *"Unless your developer account is specially provisioned by Microsoft, calls to these APIs will fail at runtime."* |
| **A custom `IPresenceWriter`** | Impossible. The registration key is owned by `NT SERVICE\TrustedInstaller`; `BUILTIN\Administrators` and `NT AUTHORITY\SYSTEM` both hold `ReadKey` only, so an elevated write fails with `0x80070005`. There is no per-user WinRT server registration to fall back on. |
| **Watching the shipped presence writer** | **This.** Free, unprivileged, modifies nothing, and leaves Xbox Live presence alone. |

The one thing all the gaming features share is the Known Game List, synced
into `HKCU\System\GameConfigStore\Children`. That data is used for naming only
— never for detection. See [Lot 3](03-game-naming.md).

## The measurements

`presence-probe activate`, unelevated:

| Step | Result |
| --- | --- |
| `RoActivateInstance` on the presence writer class | 40–43 ms, no privileges |
| The writer process appears | 6–7 ms after activation |
| It exits once the last reference is released | under 20 ms, no linger |

So the process tracks the COM reference exactly, on both edges.

Two real sessions, logged with `presence-probe watch`:

| Game | Delivery | Writer up | Writer down | Duration |
| --- | --- | --- | --- | --- |
| Starfield | Game Pass (packaged) | 17:48:29 | 17:52:03 | 3 m 34 s |
| Farming Simulator 25 | Steam (Win32) | 17:59:19 | 18:00:44 | 1 m 24 s |

Both produced **exactly one start/stop pair**. Two alt-tabs to the desktop and
back during the Starfield session did not release the reference, so the writer
does not blink on focus changes — its lifetime is the session. Starfield was
launched into the background and the writer still started, with another
application in the foreground, so detection does not depend on focus.
`GameDVR\LastGameActivity` matched the start timestamp to the second in both
sessions. As a negative control, launching and focusing Notepad spawned nothing.

The writer is a **title-level** signal, not a process-level one: Forza
restarted its own process mid-session and the writer never blinked. And the
time Windows takes to release it after a game quits is **not a property of the
title** — measured from 2.7 s to over two minutes for the same game on the
same afternoon. [Lot 1](01-console-watcher.md) has those figures and the
decision they led to.

## Why a user-session program and not a Windows service

- **Session 0 isolation.** A service cannot see the interactive desktop, and
  starting a GUI program in the user's session from one needs
  `CreateProcessAsUser` gymnastics.
- **The target programs are per-user.** They run in the user's session, with
  the user's settings.
- **No administrator rights** anywhere: not for the watcher, not for the logon
  task, not for reading the registration.
- **Easier to debug.** Run it in a console, watch the log, press Ctrl-C.

## The instrument

`presence-probe` is the measuring tool that settled the design. Nothing in
`watch` or `activate` modifies the system or needs administrator rights.

```
presence-probe status      # the registration, and whether the writer runs
presence-probe activate    # time the on-demand activation
presence-probe watch 900   # log the writer coming and going
```

`watch` polls every 200 ms so a brief launch is not missed; that is deliberate
for a measurement tool and is not how the watcher works. The tool once
carried an `install` command that tried to register itself as the writer; it
failed with access denied for the reason in the table above, and was removed
once the finding was recorded here.
