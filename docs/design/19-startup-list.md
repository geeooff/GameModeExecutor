# Lot 19 — Starting from Windows' Startup list

**Status: spike done 2026-11-16; proposed for a decision.** The maintainer
wants GameModeExecutor in *Settings → Apps → Startup*, with its switch,
instead of a logon task that page does not show. The spike adds a second
reason: Office's updates end what a logon task started, and spare what the
`Run` key started.

## Why

- **Transparency.** The Startup page, and Task Manager's, list three sources:
  the `Run` keys (`HKCU` and `HKLM\...\CurrentVersion\Run`), the Startup
  folders, and packaged apps' startup tasks. Their switch writes to
  `HKCU\...\Explorer\StartupApproved` and deletes nothing. Scheduled tasks are
  not listed, so today the watcher is invisible there.
- **Robustness, measured below.** Office Click-to-Run's update ends processes
  started by a logon task, with no trace in their logs. Office's service log
  names the step: `TaskUpdateApply::ShutdownStaleProcesses`, with
  `ForceAppShutdown (true)`, at 19:30:33 on 2026-10-11 and 12:12:39 on
  2026-10-17. What makes a process "stale" is not documented, and the watcher
  loads none of the Office files that step watches.

## What the spike measured

### Survival

From 2026-10-17 to 2026-11-16 on the maintainer's machine, two copies of
one script wrote a line a minute: one from a logon task (its parent
`svchost`), one from the `HKCU` `Run` key (its parent `explorer`). The Office
updates are MsiInstaller's reconfiguration of the Click-to-Run components
(event 1035).

| Office update | Logon task copy | `Run` copy |
| --- | --- | --- |
| 11-01, 11:29:54 | last line 11:29:08, back at the restart of 11-04 15:11 (the PC slept between) | unbroken |
| 11-06, 18:38:53 | unbroken | unbroken |
| 11-13, 11:09:37 | last line 11:08:14, back at the logon of 11-14 | unbroken |

On 11-13 the watcher died with the task copy: started 10:13:11, it next
logged a start at 17:12:17, with no session end between. It was
relaunched, and no game was on. Not every update ends task processes: the
one on 11-06 did not, and the one on 2026-10-01 spared Samsung Magician.

On the second machine, read from its log and its event log on 2026-10-23
(the times match, nothing more is proven): the watcher vanished during a
game on 10-01 (GTA V from 18:04, Office at 18:06) and on 10-10 (Overwatch
from 21:39, Office at 21:44). Office's update there on 09-22 spared it.

### What a `Run` entry gives

- **When it starts.** Explorer starts it 0 to 24 s after the logon task's
  copy, 8 s at the median, over 21 logons; the task itself waits 15 s after
  logon. Microsoft gives no promise: the system "may choose to delay the
  execution of programs in the Run key" ([Run and RunOnce Registry
  Keys](https://learn.microsoft.com/en-us/windows/win32/setupapi/run-and-runonce-registry-keys)).
- **How the page shows it.** By the executable's file description and
  publisher, not by the value's name: the spike's entry read *Windows
  PowerShell — Microsoft Corporation* (seen by the maintainer, 2026-10-17),
  and an executable without a version resource shows its file name. The
  watcher would read *GameModeExecutor watcher — Geoffrey Vancoetsem*, the
  fields `build.rs` writes today (inferred).
- **The switch.** Explorer wrote the spike's `StartupApproved\Run` entry
  itself, `02` and zeros, *on* (read 2026-11-16). Not measured: what turning
  it off does to a running watcher and at the next logon.
- The value is a command line of at most 260 characters, ignored in Safe
  Mode (Microsoft, same page). The watcher's — the twin's path and
  `--config` with the configuration's path — fits unless both paths are
  very long.

### The cost, caller by caller

The setup commands' three callers each meet the change
([Lot 13](13-updating.md) has the contract):

| Caller | Today | With the `Run` key |
| --- | --- | --- |
| `install-task` | registers the task unless one is there, then starts the watcher through it | writes the `Run` value unless one is there, removes the old task, starts the watcher (below) |
| The package | `RegisterTask` and `UnregisterTask` run those commands | the same actions; no `Registry` row, so the program stays the one writer of the value, as it must be for the zip |
| The zip's after-exit shell | calls the new binary's `install-task` | unchanged, so the verb must keep working: a 0.4 copy's shell calls it |
| `update`, a failed install | `schtasks /Run` restarts the old watcher | a 0.4 copy's shell still does that; the new version's shell starts the watcher the new way |
| `purge` | removes the task | removes the value, its `StartupApproved` entry, and a task left over |
| `status` | says whether the task is there | says whether the value is there and points at this copy |

Two edge cases, neither leaving the program unstartable:

- **A failed upgrade rolls back after the new command ran.** The value
  stays and names the same path, where the old executables are back, so
  the old version starts from `Run` at the next logon; until then, a 0.4
  shell's `schtasks /Run` finds no task. The old version's uninstall would
  not know the value; the next upgrade cleans it.
- **A 0.4 zip copy whose update fails** puts its old executables back and
  registers the task again beside the value: two starts at logon, and the
  single-instance mutex ends the second.

The verbs: `install-task` names something it would no longer do. A new
name, with `install-task` and `uninstall-task` kept as aliases for the
copies whose shells call them, is the honest shape; the name is the
maintainer's choice.

### What is lost

- The task's 15 s delay, which nothing needs; Explorer delays on its own.
- The restart on failure, which did not fire when the watcher was ended
  from outside, the only failure seen.
- `install-task --delay`, which goes with the delay.

### Open: starting the watcher right after an install

Through the task, the watcher an install starts is Task Scheduler's child,
and an Office update can end it until the next logon. Without the task, the
command must start it itself:

1. **Directly, detached.** Its parent is then the package's custom action
   or the zip's shell. Whether Office deems that "stale" is unknown, and
   so is whether Windows Installer lets it outlive the install. The
   package alone could use an asynchronous action, documented as "Execution
   continues after Windows Installer terminates" ([Custom Action Return
   Processing
   Options](https://learn.microsoft.com/en-us/windows/win32/msi/custom-action-return-processing-options)),
   but the zip and the failure path would still need the command.
2. **Through Explorer**, with the shell's documented `IShellWindows` and
   `IShellDispatch2::ShellExecute`: the watcher becomes Explorer's child,
   like a `Run` start, the parent the spike saw survive.

**Recommended: 2**, measured on all three paths. It gives the parent that
was measured rather than one that might hold.

## Done when, proposed

The watcher starts at logon from the `HKCU` `Run` key and shows on the
Startup page with its switch; an install, an upgrade and a zip update each
start it as Explorer's child and leave no logon task; a 0.4 copy updates
through both paths; `purge` leaves no value and no entry; the switch's
effect is measured; the user pages say where the switch is; and the
watcher outlives an Office update in the field.

## Beyond this program

FanControl on the maintainer's machine starts from its own logon task and
died at the same updates, which leaves the fans on whatever the hardware
does until it is started again. That is FanControl's own startup, which
needs administrator rights; noted here, not this lot's to fix.
