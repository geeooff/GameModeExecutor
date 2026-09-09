# Development plan

Working document. Updated as work lands, not written once and forgotten.

Status: `[ ]` todo · `[~]` in progress · `[x]` done · `[-]` dropped, with a reason.

A lot is **committed** when we have decided to build it, **considered** when it is
only an idea. Considered lots deliberately carry no detail: writing detail is how
scope grows.

---

## Where we are

Detection works and is validated. The watcher observes the lifetime of the Game
Bar presence writer process, which is Windows' own verdict on whether a game is
running. Two real sessions confirmed it (Starfield on Game Pass, Farming
Simulator 25 on Steam), each producing exactly one start/stop pair, with
alt-tabs not disturbing it. See the README for the measurements and for why
every other mechanism was ruled out.

What exists: configuration, action runner with placeholders, logging, single
instance, per-user logon task, the `presence-probe` measuring tool, and CI.

What has never been exercised: the actual point of the project, switching a fan
profile on a real machine.

---

## Lot 0 — Foundations · committed · `[x]` done

Goal: something that detects a game and runs commands.

- [x] Configuration model (TOML), validation, `init` / `validate`
- [x] Action runner: args, working dir, env, placeholders, no-window, wait, timeout
- [x] Console and rolling-file logging, single instance per session
- [x] Per-user logon task via `schtasks`, no elevation
- [x] Detection by presence writer lifetime: idle poll, then park on the process handle
- [x] Registration resolved from the registry, matched on the full image path
- [x] Game naming from the Known Game List, Win32 paths and packaged AUMIDs
- [x] `presence-probe` (`status`, `activate`, `watch`)
- [x] CI on windows-latest: fmt, clippy `-D warnings`, tests, release build, artifacts

Done when: a simulated game start and stop drives the configured actions.
**Verified** 2026-09-09 by activating the presence writer directly: start fired
975 ms after it appeared (1 s idle poll), stop 1005 ms after it exited.

---

## Lot 1 — Field validation · committed · `[ ]` next

Goal: prove the real use case on the real machine. Almost no code; the value is
in finding out what we got wrong.

- [ ] Verify FanControl's command line for switching profiles
- [ ] Install FanControl and create two profiles (gaming / silent)
- [ ] Write the real `config.toml` and run `validate`
- [ ] Play one session with `run` in the foreground, read the log
- [ ] Confirm the game was named correctly in the placeholders
- [ ] Install the logon task and confirm it survives a reboot
- [ ] Fix whatever this turns up; correct the README example if the CLI differs

Done when: a fan profile visibly switches on game start and reverts on game stop,
started automatically at logon, with nothing typed by hand.

Notes and risks:

- **FanControl is not installed on this machine.** The `-c <profile>` form used
  in `config.example.toml` and the README is an unverified assumption carried
  since the first sketch. It has to be checked against the real binary before it
  is presented as an example.
- Placeholders can come out empty for a title Windows tracks but does not
  describe. That is expected and does not block the actions; worth confirming
  once in the field.
- Until this lot passes, everything else is polish on an unproven product.

---

## Lot 2 — Notification area icon · committed · `[ ]`

Goal: see at a glance whether a game is detected, and be able to quit the
watcher without the task manager.

Impact was measured before committing: `user32`, `gdi32`, `shell32` and
`combase` are already loaded, so no new DLL. Expect well under 1 MB of extra
private bytes, no measurable CPU, and +30–60 KB of binary.

- [ ] **2a** Invert the threading: message loop on the main thread, engine on a
      worker. No visible change; existing behaviour must be identical.
- [ ] **2b** Two placeholder `.ico` files, multi-size (16/20/24/32) for HiDPI:
      one idle, one game-detected
- [ ] **2c** Embed them, add the icon, tooltip showing the detected game
- [ ] **2d** Re-add the icon on `TaskbarCreated` (Explorer restart)
- [ ] **2e** Right-click menu with Quit
- [ ] **2f** `--no-tray` for pure console use

Done when: the icon reflects the state through a full game session, survives
killing and restarting Explorer, and Quit shuts the watcher down cleanly.

Notes and risks:

- 2a is the only structurally invasive step: the tray window needs a thread that
  pumps messages, and today the main thread blocks in `WaitForMultipleObjects`.
  Engine stays pure and testable; the tray lives in its own module.
- 2e is not scope creep. An icon that does nothing on click reads as stuck, and
  an instance started by the logon task would otherwise have no way out.
- Avoid `LoadIconMetric`: it pulls `comctl32.dll`, which is not loaded today.
  Ship the right sizes in the `.ico` instead.
- Icon embedding: `winresource` as a build-dependency also gives the exe a
  proper Explorer icon. The alternative, `include_bytes!` plus
  `CreateIconFromResourceEx`, keeps build dependencies at zero but costs about
  40 lines and no exe icon. Decide in 2c.
- Console subsystem is kept, so `run --hidden` still flashes a console briefly at
  logon. Switching to the `windows` subsystem would kill `status` and `check`
  output. Not worth it yet.

---

## Lot 3 — Distribution · committed · `[ ]`

Goal: someone other than us can install it.

- [ ] Create the public GitHub repository and push
- [ ] Release workflow: on a `v*` tag, build and attach the executables
- [ ] Version and description metadata in the exe
- [ ] README install section pointing at the release, not at `cargo build`

Done when: a fresh machine can download one archive, run `init`, and be watching.

---

## Lot 4 — Comfort · considered

Only after Lot 1 has told us what actually matters in use.

- Open the log folder and the config from the tray menu
- Pause and resume detection
- A toast when a profile switches

---

## Lot 5 — Robustness · considered

- Run the stop actions on logoff and shutdown. Becomes cheap once Lot 2 gives us
  a window: `WM_QUERYENDSESSION` / `WM_ENDSESSION`.
- Reload the configuration without restarting
- Behaviour across two games launched back to back

---

## Non-goals

Recorded so they stop coming back:

- No FanControl-specific integration. The program runs executables; that is all.
- No Windows service. Session 0 cannot see the desktop or the user's apps.
- No configuration GUI.
- No allow-list of game executables, and no heuristics that guess at what a game
  is. Detection stays Windows' verdict.
- No telemetry, no network access.

---

## Assumptions still to verify

| Assumption | Status |
| --- | --- |
| FanControl switches profiles with `-c <profile>` | **unverified**, and FanControl is not installed on the dev machine |
| The presence writer is activated for games only | Notepad was a clean negative control; not proof for every application |
| Naming covers the titles actually played | Two of two named so far, once packaged titles were supported |
| The writer never blinks mid-session | Holds over two sessions including alt-tabs; `watch` polls at 100 ms so a sub-100 ms dip could hide |

---

## Journal

**2026-09-09** — Detection settled. Ruled out the Game Mode APIs (deprecated
since 1809, callable only from inside the game), Xbox Mode (a shell mode with
private APIs), and `GameList` (restricted capability). A custom `IPresenceWriter`
turned out to be impossible: the registration key is owned by TrustedInstaller
and neither Administrators nor SYSTEM can write it. Observing the shipped writer
instead works and costs nothing. Measured its activation (40 ms) and exit
(under 20 ms after release), then validated it over two real game sessions.
Added packaged-title naming after Starfield turned out to carry no executable
path at all. Removed the process allow-list and the interim full-screen
detector.
