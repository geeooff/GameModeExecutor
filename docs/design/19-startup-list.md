# Lot 19 — Starting from Windows' Startup list

**Status: proposed 2026-10-17, a spike first.** The maintainer wants
GameModeExecutor in *Settings → Apps → Startup*, with its switch, instead of
a logon task that page does not show; to be taken soon unless another lot
comes first.

## Why

- **Transparency.** The Startup page, and Task Manager's, list three sources:
  the `Run` keys (`HKCU` and `HKLM\...\CurrentVersion\Run`), the Startup
  folders, and packaged apps' startup tasks. Their switch writes to
  `HKCU\...\Explorer\StartupApproved` and deletes nothing. Scheduled tasks are
  not listed, so today the watcher is invisible there.
- **Robustness, to be measured.** At Office Click-to-Run's updates of
  2026-10-11 and 2026-10-17, every process started by a logon task on the
  maintainer's machine died — the watcher, FanControl, Samsung Magician — with
  exit code 1 and no trace, and every `Run`-key program survived. Office's
  service log names the step: `TaskUpdateApply::ShutdownStaleProcesses`, with
  `ForceAppShutdown (true)`, at 19:30:33 and 12:12:39. What makes a process
  "stale" is not documented, and the watcher loads none of the Office files
  that step watches. The second machine runs the same Office.

## The spike

1. **Survival.** At the next Office update, a long-lived dummy started by a
   logon task beside one started from `Run`; the Office service log and
   Task Scheduler's result say which ended. Set up on the maintainer's
   machine on 2026-10-17, with approval: two copies of one script writing a
   line a minute, so each log dates its death.
2. **What a `Run` entry gives.** The delay after logon (Explorer staggers
   startup entries), the switch's effect on a running watcher and at the next
   logon, the name and icon the page shows.
3. **The cost of the change** across the setup commands' three callers: the
   package (a per-user `Registry` row, as the Start menu shortcut already
   has), the zip's after-exit shell, `purge`; an upgrade that removes the old
   task; `status`; the documentation.
4. **What is lost.** The task's 15 s delay and its restart on failure, which
   did not fire when the watcher was killed.

**Done when** is written after the spike, if the lot is decided.
