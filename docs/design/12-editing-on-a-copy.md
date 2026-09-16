# Lot 12 — Editing the configuration without breaking it

**Status: proposed.** On the `visudo` / `git commit` / `systemctl edit`
pattern: *Edit configuration* opens the editor on a **copy**, and the real
file only ever receives content that validated. [Lot 9](09-robustness.md)'s
strict disable is what makes this worth having, and this is what makes that
strict disable comfortable: through the program's own path, an invalid file on
disk cannot happen.

## The signal is the file, not the editor

The first idea — promote when the editor process exits — is not reliable on
Windows and cannot be made so. `ShellExecuteEx` does return a process handle,
but most editors are single-instance: `code file.toml` hands the file to the
running window and returns at once, and Windows 11's Notepad does the same
since it became a packaged, tabbed app — the `notepad.exe` launched is a stub
that exits immediately. Git's answer is `--wait` flags the user configures in
`core.editor`; "open with the associated application" has no such convention.

So instead:

- Copy the configuration to `%LOCALAPPDATA%\GameModeExecutor\config.editing.toml`
  — local, because the real file may sit in `%APPDATA%` and roam, and a
  half-edited copy travelling to another machine would be absurd — and open
  *that*.
- Watch it with Lot 9's watcher. Every save is a candidate: valid, and it is
  promoted onto the real file (write `.new`, rename over — atomic); invalid,
  and the tray shows the fault exactly as Lot 9 does, while the real file
  stays untouched and valid. Closing the editor without a valid save is
  "cancel". Nothing needs to know when editing ends.
- No prompt. The tray *is* the prompt: the icon says what is being written is
  invalid, the menu says where, the user fixes or closes.

## Consequences worth having

The lot is a few dozen lines if Lot 9's watcher takes the path and the apply
action as parameters. [Lot 10](10-configuration-window.md)'s window becomes a
second client of the same stage-validate-promote path rather than a third way
of writing the file. A stale `config.editing.toml` found at start is tidied
away.

**Known wrinkle.** The editor's title bar shows `config.editing.toml`, not
`config.toml`. `visudo` shows `sudoers.tmp`; it surprises once.
