# GameModeExecutor

A small Windows watcher, written in Rust, that runs the executables you configure
when a game starts and when it stops.

It knows nothing about any particular tool. It just launches programs with
arguments, which is enough to drive
[FanControl](https://github.com/Rem0o/FanControl.Releases) profiles, an RGB CLI, a
power plan switch, or anything else that has a command line.

```
game detected  ->  [[on_game_start]]  ->  FanControl.exe -c gaming.json
game gone      ->  [[on_game_stop]]   ->  FanControl.exe -c silent.json
```

**Status: work in progress.** The action runner and the configuration are done.
The detection side is being rebuilt on top of Windows' own game detection rather
than a hand-maintained list of executables — see below.

## How Windows detects a game, and what is usable

The goal is to reuse whatever Windows already knows, instead of maintaining an
allow-list of game executables. Here is what was found, and what it is worth.

### Game Mode

An OS behaviour applied to the foreground game: higher CPU scheduling priority,
no driver installs or restart prompts mid-session. Toggled by
`HKCU\Software\Microsoft\GameBar\AutoGameModeEnabled`.

**Not usable as a signal.** The Game Mode APIs (`expandedresources.h`,
`HasExpandedResources`) are
[deprecated since Windows 10 1809](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/gamemode/game-mode-portal),
and were only ever callable from inside the game process. There is no supported
way to ask the system whether Game Mode is currently active.

### Xbox Mode (the Xbox full screen experience)

Internally "Gaming Posture" (`Windows.Internal.GamingExperiences.Posture.*`,
`HKCU\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration\GamingHomeApp`).
It replaces the shell with the Xbox app as the home experience, defers Explorer
subsystems and startup apps, and is
[rolling out to desktops](https://blogs.windows.com/windowsexperience/2025/11/24/xbox-full-screen-gaming-experience-now-available-for-windows-11-handhelds-and-in-preview-for-pcs/).

**Not usable as a signal.** It is a shell mode, not a detector, and its APIs are
`Windows.Internal.*` — private.

### GameList (`Windows.Gaming.Preview.GamesEnumeration`)

The WinRT API that enumerates the games Windows knows about.

**Not usable.** It requires the restricted `gameList` capability, and the
documentation is explicit: *"Unless your developer account is specially
provisioned by Microsoft, calls to these APIs will fail at runtime."*

### What the three features actually share

The Known Game List, synced by Windows into the registry
(`HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\GameDVR\KGLRevision`) and
expanded under `HKCU\System\GameConfigStore\Children`. Each entry describes one
title, some with `MatchedExeFullPath`, some with `ExeParentDirectory`, some with
an Xbox `TitleId`, and many with a `LastAccessed` timestamp that Windows updates
when the game runs. That timestamp is mirrored globally in
`HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\GameDVR\LastGameActivity`.

This is readable, inert, needs no privileges — but it is undocumented, and it
describes what Windows knows rather than telling us when something happens.

### Game Bar Presence Writer — the one supported push mechanism

[Documented by Microsoft](https://learn.microsoft.com/en-us/windows/win32/devnotes/gamebar-presencewriter):
an out-of-proc WinRT server that Windows calls when *it* decides a game's
presence changed.

```
IPresenceWriter::UpdatePresence(hWnd, event, appId, appIdType)
    event = GotFocus | LostFocus | AppClose
    appId = AUMID, or Xbox Live TitleId
```

Registered by pointing this value at your executable:

```
HKLM\SOFTWARE\Microsoft\WindowsRuntime\Server\
     Windows.Gaming.GameBar.Internal.PresenceWriterServer\ExePath
```

**Writing a custom one is not possible.** The registration key is owned by
`NT SERVICE\TrustedInstaller`; `BUILTIN\Administrators` and `NT AUTHORITY\SYSTEM`
both hold `ReadKey` only, so an elevated write fails with `0x80070005` and
running as SYSTEM would too. There is no per-user WinRT server registration to
fall back on (`HKCU\Software\Microsoft\WindowsRuntime`,
`HKCU\Software\Classes\ActivatableClasses` and
`HKCU\Software\Classes\WindowsRuntime` are all absent). Registering one would
mean taking ownership of a protected system key, which Windows servicing can
silently reset.

### Watching the writer instead of replacing it

The shipped `GameBarPresenceWriter.exe` is an on-demand WinRT server, so its
*process lifetime* is observable without touching anything. Measured with
`presence-probe activate` on Windows 11 25H2 (26200.9445), unelevated:

| Step | Result |
| --- | --- |
| `RoActivateInstance` on the class | succeeded in 43 ms, no privileges needed |
| `GameBarPresenceWriter.exe` appears | 7 ms after activation |
| Process exits after the last reference is released | under 20 ms, no linger |

So the process tracks the COM reference exactly, on both edges. And it is
selective: launching and focusing Notepad (`presence-probe watch`) spawned
nothing at all.

That makes "is `GameBarPresenceWriter.exe` running?" a candidate signal that
costs nothing, needs no privileges, modifies nothing, and leaves Xbox Live
presence alone — while still being Windows' own verdict on what a game is.

**Still unproven, and only a real game can settle it:** whether Windows holds
its reference for the whole session (the process stays up, and its lifetime is
the signal) or activates, calls `UpdatePresence`, and releases immediately (the
process only blinks at each focus change, which is far less useful).

## The probe

`presence-probe` is the measuring instrument. Nothing in `watch` or `activate`
modifies the system or needs admin.

```bash
cargo build --release
```

```bash
target\release\presence-probe.exe status      # show the registration
target\release\presence-probe.exe activate    # time the on-demand activation
target\release\presence-probe.exe watch 900   # log the writer coming and going
```

To settle the open question: start `watch`, play a game, alt-tab out and back a
few times, quit the game, then read `presence-probe.log` next to the executable.

`watch` polls every 100 ms so a brief launch is not missed. That is deliberate
for a measurement tool and is not how the shipping detector would work.

`install` / `uninstall` remain in the probe for the record; `install` fails with
access denied on current Windows, for the reason given above.

## Why a console app and not a Windows service

The watcher runs as a normal user-session program, started at logon by a
scheduled task. That is a deliberate choice:

- **Session 0 isolation.** A service cannot see the interactive desktop, and
  starting a GUI program such as FanControl in the user's session would need
  `CreateProcessAsUser` gymnastics.
- **The target apps are per-user.** FanControl runs in your session, with your
  profile and your settings. The thing that drives it belongs there too.
- **No admin rights** for the watcher itself; a per-user logon task is enough.
- **Easier to debug.** Run it in a console, watch the log, hit Ctrl-C.

## Commands

| Command | What it does |
| --- | --- |
| `run [--hidden]` | Watch and react. `--hidden` hides the console and logs to a file. This is the default command. |
| `status` | Print what the detector currently sees, then exit. |
| `trigger start\|stop` | Run one set of actions immediately, ignoring detection. Handy to test your commands. |
| `validate` | Parse and check the configuration. |
| `init [--force]` | Write a starter configuration file. |
| `install-task [--delay HHHH:MM]` | Register a per-user logon task that runs the watcher hidden. |
| `uninstall-task` | Remove that task. |

Global options: `--config <PATH>`, `--log-level <LEVEL>`.

Without `--config`, the file is looked up next to the executable first
(`config.toml`, portable install), then in
`%APPDATA%\GameModeExecutor\config.toml`.

## Detection today

The interim detector is the Windows shell notification state
(`SHQueryUserNotificationState`), which reports whether a full-screen application
owns the desktop. It is a public API and needs no privileges, but it is a
heuristic: it also fires on full-screen video players. It will be replaced or
demoted once the probe results are in.

`start_delay` and `stop_delay` debounce both directions, so a loading screen or a
quick alt-tab does not flip your fan profile back and forth.

## Actions

Each `[[on_game_start]]` / `[[on_game_stop]]` entry starts one executable. The
following placeholders are substituted in `program`, `args`, `working_dir` and
`env` values:

| Placeholder | Value |
| --- | --- |
| `{event}` | `game_start` or `game_stop` |
| `{process_name}` | e.g. `cs2.exe` |
| `{process_id}` | e.g. `12345` |
| `{process_path}` | full image path, when readable |

See [`config.example.toml`](config.example.toml) for the full annotated reference.

## Notes and limits

- `stop_actions_on_exit` runs the stop actions when the watcher shuts down
  gracefully (Ctrl-C). A task killed outright by the scheduler at logoff does not
  get that chance.
- Only one instance runs per session; a second one exits immediately.

## License

MIT. See [LICENSE](LICENSE).
