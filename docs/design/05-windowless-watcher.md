# Lot 5 — A Windows program with no window

**Goal.** The same program, minus the console. Nothing on screen — not a
window, not an icon, not a flash at logon — and behaviour otherwise identical.

**Done when:** the logon task starts the watcher with nothing on screen, every
CLI command behaves exactly as before from a terminal, and a logoff during a
game restores the profile. The first two hold. The third does not, and cannot
by this mechanism — measured on 2026-09-16, below, and taken up in
[Lot 9](09-robustness.md).

- [x] The watcher built as a Windows-subsystem binary
- [x] Every CLI command kept, exit codes included — two binaries, see below
- [x] Threading inverted: message loop on the main thread, engine on a worker
- [x] A hidden top-level window whose procedure answers `WM_QUERYENDSESSION` and `WM_ENDSESSION`
- [x] `install-task` points the logon task at the windowless binary and stores an absolute configuration path
- [x] Verified: the task starts it with no window; `status`, `validate` and the exit codes are unchanged from a terminal; the session-end sequence drives a clean shutdown
- [x] A real logoff while a game is running — the handshake worked, the command could not start

## Two binaries, the `w` convention

| Binary | Subsystem | For |
| --- | --- | --- |
| `gamemode-executor.exe` | `WINDOWS_CUI` | everything typed: `status`, `validate`, `check`, `trigger`, `init`, `install-task`, and `run` |
| `gamemode-executorw.exe` | `WINDOWS_GUI` | watching, and nothing else. What the logon task runs. |

Both are a few lines over the same library; `src/service.rs` holds the one
implementation of "run the watcher" they share. Verified by reading the
subsystem field out of each PE header rather than by trusting build settings.

Three ways to keep the CLI were weighed:

1. **Two binaries** — how Python ships `python.exe` and `pythonw.exe`, and
   Perl, because the two subsystems cannot share one file. Nothing to hack: the
   shell keeps waiting on the console binary, so exit codes, pipes,
   redirection and colours keep working. Cost: one more file.
2. **One Windows-subsystem binary with `AttachConsole(ATTACH_PARENT_PROCESS)`.**
   A shell does not wait for a GUI-subsystem process: the prompt returns before
   the output, which prints over it, and `$LASTEXITCODE` / `%ERRORLEVEL%` are
   not set — which breaks `validate` in any script, silently. The one that
   breaks a promise.
3. **Stay a console program and `FreeConsole()` at startup.** The console is
   allocated before `main` runs, so a window flashes at logon before it goes.
   Does not meet "nothing on screen".

The first was taken.

## The window, and why it is here rather than with the icon

A Windows-subsystem process with no window hears nothing from the shell and
is simply terminated at logoff. A hidden **top-level** window — created, never
shown — receives `WM_QUERYENDSESSION`, `WM_ENDSESSION` and `WM_SETTINGCHANGE`;
a message-only window receives none of those broadcasts. It needs a thread
pumping messages, hence the inversion: message loop on the main thread, engine
on a worker. The icon in Lot 6 hangs off the same window, but the window was
proved with nothing on it first.

It was built to preserve what the console build was believed to do at logoff:
`ctrlc`'s Windows handler fires on every control event, so the stop commands
were *started*. That behaviour had been inferred from the handler's signature
and never measured. The real logoff below showed that starting the stop
commands is not the same as running them, and the console build would very
probably have failed the same way. The window is still worth having — the
icon, the theme broadcasts and the handshake hang off it — but the reason first
given for it was hollow.

### Verifying the session-end path without logging off

`WM_QUERYENDSESSION` and `WM_ENDSESSION` were sent to the running watcher's own
window. It answered 1, released `WM_ENDSESSION` in 7 ms, logged `Stopped` and
exited on its own.

`FindWindow` could not find the window, which looked like a defect and was
not: it resolves a class name through the global atom table, and a class
registered with `RegisterClassEx` is local to its process. `EnumWindows` plus
`GetClassName` asks each window directly.

### The real logoff, 2026-09-16

Starfield running, sign-out at 00:18:00 by the clock. Instrumentation added an
hour earlier is the only reason this reads as anything but a log that stops:

```
00:18:01.220  Windows asked to end the session, so the watcher starts stopping now
00:18:01.220  Stopping while a game is running, so the stop commands run now
00:18:01.221  Game no longer detected: Starfield.exe
00:18:01.221  Starting a command  schtasks.exe /Run /TN GameModeExecutor\FanControl Quiet
00:18:01.330  `FanControl - Quiet profile` finished  status=exit code: 0xc0000142
00:18:01.336  The stop commands finished, the session may end  waited=61.8ms
00:18:38.275  GameModeExecutor 0.1.0 starting                      <- next logon
```

Every step the program is responsible for happened, and fast: asked at
`.220`, answered and stopping in the same millisecond, first command started
1 ms later, the handshake released in 62 ms. And the fan profile stayed on
*Game* until `trigger stop` was run by hand.

`0xC0000142` is `STATUS_DLL_INIT_FAILED`: the child process was created but a
DLL's initialisation failed — user32 cannot connect to a window station being
torn down, and a console child additionally needs a conhost that cannot start
either. The process already running kept running for as long as it liked. The
process *born* during logoff was stillborn.

The timing is the point. `WM_QUERYENDSESSION` is the **first** thing any
application hears about a session ending, and the command was started one
millisecond after it. There is no earlier moment to be had. **No design that
starts a process at logoff can restore the profile**, on this Windows build at
least. Winlogon's own timeline agrees: event 7002 at 00:18:07, logon at
00:18:22, watcher back at 00:18:38.

What does work is remembering that a session is open and finishing it at the
next start — [Lot 9](09-robustness.md).

## Kept out of this lot

No icon, no menu, nothing drawn. No `ShutdownBlockReasonCreate` either
("Restoring the fan profile…" on Windows' shutdown screen) — worth having,
belongs with the rest of the shutdown work in Lot 9.
