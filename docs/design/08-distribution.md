# Lot 8 — Distribution

**Status: in progress since 2026-09-17.** Already there: `scripts/build.ps1`
runs the whole checklist and produces the zip archive in `dist/`,
`.vscode/tasks.json` drives it, and the repository is public with CI green
on a stock runner. The script was written first on purpose — a release that
cannot be made by hand is not one CI can make either. What is left, in the
order it is taken:

- [x] Create the public GitHub repository and push — done 2026-09-17, with approval
- [x] The three measurements below, on a minimal package, before any table is written — done 2026-09-17
- [x] `VERSIONINFO` metadata in the executables, through the same `rc.exe` step that embeds the icon — done 2026-09-17, checked by the checklist
- [x] An MSI, per-user, into `%LOCALAPPDATA%\Programs\GameModeExecutor`, with the user's files outside its components — built 2026-09-17, ICE clean, the round trip measured below
- [x] `gamemode-executor purge`, the same command in every mode — built 2026-09-17, below
- [x] The whole delivery chain, unattended: pushing a `vX.Y.Z` tag makes CI run
  the checklist, build the **MSI** and the **zip archive**, and publish a
  GitHub Release carrying both with their SHA-256 — nothing built or uploaded
  by hand. Written 2026-09-17 (`release.yml`, `scripts/release-notes.ps1`);
  its first run is the first tag
- [x] The documentation: *Getting started* and the README point at the release rather than at `cargo build`, the reference gains `purge`, *How it works* gains removal — 2026-09-17
- [ ] Verified in the field: the MSI on two machines, one real upgrade, one purge round trip

**Done when** a tag alone produces a release a stranger can install from, and
the two artefacts on it were built by the workflow from that tag's commit.
Decided 2026-09-17: two artefacts, MSI and zip, not one or the other — the
installer for the ordinary case, the zip for the person who wants no
installer at all.

## The zip is not a portable build

A portable program lives entirely in its own folder. Windows Terminal is the
reference: a file named `.portable` next to the executable switches it to
that mode, and its settings then stay in the folder instead of going under
the profile. This program writes to standard places however it was
installed — the session marker under `%LOCALAPPDATA%`, the configuration in
the roaming profile unless it sits next to the executables, the log wherever
the configuration says, and scheduled tasks. Calling the archive *portable*
promised something it does not do, so the word was retired from the
repository on 2026-09-17: the archive is the **zip**, and a copy unpacked
from it is **hand-installed**.

A portable mode proper — everything in one folder, nothing elsewhere, and
therefore no logon task — would be a lot of its own. It is not numbered; it
waits for someone to need it.

## Three ways in, one way out

| Mode | Who owns the executables | Where the data lives |
| --- | --- | --- |
| MSI | Windows Installer | the standard places above |
| Zip | the user | the standard places above |
| Portable, if it comes | the user | the program's folder and nowhere else |

**Decided 2026-09-17: whichever the mode, the program removes every trace of
itself on request** — configuration, executables, logs, marker, scheduled
tasks, all of it — and nothing removes any of it without being asked.

- **Upgrades never purge.** An upgrade replaces the executables and nothing
  else. For the MSI that is a constraint on the package, not a courtesy:
  `config.toml`, the log and the marker are user data, not components, so no
  repair, upgrade or uninstall can touch them. The installer writes no
  configuration; `init` does, when asked.
- **Uninstalling does not purge either.** The MSI's uninstall removes what
  the MSI installed — the executables — which is what Windows applications
  ordinarily do. The zip has no uninstaller; the user deletes the folder.
- **The purge is a command, `gamemode-executor purge`, not a script.** The
  program already knows every location — where the configuration was found,
  where the log is written, the local folder, the task names — and a script
  would carry a second copy of that knowledge, which drifts. It runs
  **before** the uninstall, in every mode. The shape proposed: refuse while
  a game session is open, because leaving a machine on its gaming
  configuration with nothing left to restore it is the one thing this must
  not do; stop the watcher; remove its own scheduled task; remove
  `%LOCALAPPDATA%\GameModeExecutor` and `%APPDATA%\GameModeExecutor`, the
  configuration wherever it was found, the log wherever it was written; and
  last the executables — a process cannot delete itself, so a hand-installed
  copy hands that step to a detached shell that waits for it to exit, and an
  MSI install runs `msiexec /x` so Windows Installer's registration goes
  too. It lists what it is about to remove and asks once; `--yes` is for
  scripts.
- **It removes what it recognises as its own and leaves the rest, saying
  what it left.** The program does not know which recipe the user followed,
  and must not: a task in the `\GameModeExecutor` folder that it did not
  register stays, and the folder stays with it; a log folder that holds
  anything but its log stays. The recipes' tasks are the recipes' business —
  `install-tasks.ps1` is a convenience script, and an `uninstall-tasks.ps1`
  beside it, elevating the same way, is the correct counterpart. Decided
  2026-09-17.
- **A prompt at MSI uninstall time** — *also remove the configuration, logs
  and tasks?* — is the obvious UX, and exactly where a hand-authored MSI gets
  expensive: a dialog is the Dialog, Control and ControlEvent tables plus a
  custom action that calls `purge`. The lot measures that cost against the
  alternative, no dialog and the documented command, and decides with the
  number in hand.

**Built 2026-09-17, `src/purge.rs`.** The plan is computed from a `Layout`
that says what exists, separately from discovering the machine, so seven
tests drive it on scratch folders — including the hand-installed case,
where a detached `cmd.exe` deletes the executables after a pause and the
test waits it out. One thing that cost an hour: `std::process::Command`
escapes the quotes around paths as `\"` for `CommandLineToArgvW`, which
`cmd.exe` does not read, so `del` looked for a file that did not exist and
said nothing; the command line is passed raw. The installed case is told
apart by `MsiEnumRelatedProducts` on the upgrade code, and a test checks
the Rust constant against the one `scripts/msi.ps1` writes. The watcher is
stopped with `WM_CLOSE` on its session window, found by `EnumWindows`
because `FindWindow` cannot see a class another process registered, and
the single-instance mutex says when it has gone. Still to measure in the
field: the purge itself on a real installation, and the cost of an
uninstall-time prompt, in tables — not built, and not missed so far.

## Versioning

Decided 2026-09-17, when the question came up before publication: why the
version had sat at 0.1.0 through eleven lots, and whether the first public
release should be 1.0.0.

**A version is a property of a release, not of a commit.** Its job is to tell
someone holding an earlier version what changed; through development nobody
held any, and `--version` already names every build to the commit. So nothing
was bumped because nothing was released — SemVer's own reading of major
version zero, *initial development, anything may change*. Not an omission.

**The public API of an application is its user-facing contract**, and that is
what the major number protects here: the schema of `config.toml`, the
commands and their exit codes, the names of the scheduled tasks, where the
marker and the log live, and the `info` lines of the log.

**The rules, from the first release on:**

- The repository going public is not a release. It stays 0.1.0.
- The first GitHub Release is **0.1.0** and comes with this lot — a release
  *is* the delivery chain.
- **1.0.0 when three things are true:** the installer exists (this lot), an
  invalid configuration shows in the icon ([Lot 9](09-robustness.md)), and
  the `config.toml` schema is declared stable. Not before: `1.0.0` next to a
  design record that says *partly done* would contradict itself, and the
  SemVer argument for it — *if it is in production it is 1.0* — carries
  little weight with two machines.
- Patch for a fix that leaves the contract alone; minor for a feature, and,
  while in 0.x, for a contract change said in the release notes; major
  reserved for 1.0.0 and then for any break of the contract.
- **The bump happens in the release commit**, the one the `vX.Y.Z` tag names
  and the workflow builds. Between releases the number does not move; the
  commit stamp tells builds apart. One place to change, `Cargo.toml` — the
  zip's name derives from it, and the version written into the README's
  status line goes when this lot lands, so there is no second place.
- Tags are `vX.Y.Z`, created by the release step, never by hand during
  development. The `lot-N` tags that had accumulated pointed at a history the
  rewrite replaced and were deleted before publication.

## The commit is compiled into the binaries

Done ahead of the lot, because the tray menu's documentation entry needed it.
`build.rs` emits the commit through `cargo:rustc-env`; `src/build_info.rs`
exposes it as constants and nothing else. Every branch — commit known or not,
which reference the link should use — is taken in `build.rs`, so the program
side has no runtime assembly at all, which is also what clap requires of a
version string.

- `-V` prints `0.1.0 (de538e33-dirty)`.
- `--version` adds the full commit, the repository and the documentation link.
- `status` prints the same three lines first, before anything it reports.
- The startup log line carries the commit as a field, visible at `debug`.
- No git, no failure: the commit reads `unknown (built outside a git checkout)`
  and the link falls back to `main`. A source archive compiles fine.
- **`scripts/build.ps1 release` refuses a dirty tree**, and checks the
  stamped commit is the commit being built. A binary built from uncommitted
  changes would name a commit that does not contain what it ships.

`build.rs` watches `.git/HEAD`, the ref it names, and `packed-refs` — because
`.git/HEAD` alone does not change on commit, and a stale stamp once shipped.

## Documentation is linked, not shipped

**Decided: the installer carries no `docs/` tree.** It carries a link naming
the exact commit the binaries were built from:

```
https://github.com/Geeooff/GameModeExecutor/blob/<commit>/docs/getting-started.md
```

A branch link would rot — it would show whatever `main` says today, which may
describe a version the user is not running. A commit link is the documentation
*for the thing they have*, permanently, which is worth more than a local copy
that goes stale the moment they update.

Not in `config.toml`, though. That file belongs to the user: they edit it, they
keep it across upgrades, and a build-time constant in it would be wrong rather
than merely old the first time they replace the executables without replacing
the file. The commit lives in the binary, where it cannot desynchronise from
the code it describes.

The link resolves only once the repository is public. Until then it is correct
and unreachable, which is the right way round.

## The installer, and the MSI question settled properly

The right place is settled: `%LOCALAPPDATA%\Programs\GameModeExecutor`,
per-user, writable, no elevation.

An earlier version of this page said MSI was the wrong format because per-user
installs are awkward, discouraged, and prompt for elevation anyway. That was
wrong on all three counts, and is corrected here rather than quietly deleted.
Microsoft documents the scenario as **Single Package Authoring**, a
dual-purpose Windows Installer 5.0 package whose stated purpose is to "remove
UAC credential prompts from per-user installations". `ALLUSERS=2` with
`MSIINSTALLPERUSER=1` makes per-user the default, and in that context
`ProgramFilesFolder` redirects to `%LocalAppData%\Programs` — the folder chosen
here for an entirely separate reason. The documented constraints on such a
package — no elevated custom actions, no writes to global folders, no GAC, no
services — are ones this program already meets.

### Two candidates, and the one named

| | Inno Setup | MSI, Windows SDK only |
| --- | --- | --- |
| Third-party dependency | one, free, no strings | **none** |
| Authoring | a short, readable script | IDT table files imported with `msidb`, File table filled by `Msifiler` |
| Effort | low | real, and proportional to the number of files |
| Validation of the package | none | `ICE105` checks a dual-purpose package is valid |
| Install context | the installer's own bookkeeping | Windows records per-user vs per-machine itself |
| Uninstall | its own uninstaller | Windows Installer's, transactional |
| Precedent | VS Code's per-user installer | `PUASample1.msi` ships with the SDK |
| `winget` | supported (`InstallerType: inno`) | native |

What keeps the SDK route affordable is the file count: hand-authoring MSI
tables scales badly with files and well without them, and this package has
under ten.

**Named 2026-09-17: the installer is an MSI.** The comparison stays as the
record of what it was weighed against. How the MSI is authored — the SDK
tools, given what is said of WiX below — is confirmed when the lot starts
and the tables are actually written, not before.

**Measured 2026-09-17, three yeses.** A minimal package — one text file,
no UI, built in PowerShell with nothing but Windows Installer's own COM
automation and `makecab` — was installed, upgraded and removed from an
unelevated shell, `/passive`, with a verbose log each time:

| | Result | The log's word for it |
| --- | --- | --- |
| Install 0.1.0 | exit 0 in 7 s, no prompt; the file in `%LOCALAPPDATA%\Programs\<name>\`, the product registered per-user (`AssignmentType=0`) and listed in *Programs and Features* | `MSI_LUA: Package is marked as LUA installation capable with no elevation required` |
| Upgrade to 0.2.0 | exit 0 in 6 s, no prompt; file replaced, the old product gone, one product left at 0.2.0 | `Nested installation UAC elevation tracks that of parent (is not elevated)` — `RemoveExistingProducts` at 1510 removed 0.1.0 first |
| Uninstall | exit 0 in 6 s, no prompt; folder gone, registration gone; the neighbouring folders, the watcher's install and its scheduled tasks untouched | `Removal completed successfully` |

Two things the documentation had not made plain. **The summary stream's
"elevated privileges not required" bit (WordCount bit 3) is the whole
mechanism**: with it set, Windows Installer treats the package as per-user
outright, redirects `ProgramFilesFolder` to `%LOCALAPPDATA%\Programs`, and
logs `MSIINSTALLPERUSER property is not valid for UAC compliant package.
Ignoring` — so `ALLUSERS=2` and `MSIINSTALLPERUSER=1`, the dual-purpose
recipe, are not needed for a program with no per-machine story, and the
package is simpler without them. And **no SDK tool is needed to build the
database**: the COM automation creates tables, inserts rows and embeds the
cabinet, which means the release script can produce the MSI on a stock
runner the same way it produces the zip. `MsiDb`, `MsiFiler` and `Orca`
remain what they are, tools to inspect one. What the probe did not do and
the real package must: carry versioned files with `VERSIONINFO`, and pass
ICE validation (`MsiVal2`, from the SDK).

The choice is closed: **MSI, authored from PowerShell through Windows
Installer's automation, per-user by the summary bit.**

Both candidates can offer per-user *or* per-machine from one installer, but
**this program has no per-machine story**: the logon task, the configuration
and the log are all per-user. A per-machine install would still leave every
user to run `install-task`, and charge an elevation prompt for the privilege.
Per-user only — in Inno Setup that is `PrivilegesRequired=lowest` and
`DefaultDirName={userpf}\GameModeExecutor`.

Ruled out, so they are not reconsidered from scratch:

| Ruled out | Why |
| --- | --- |
| **NSIS** | The same ground as Inno Setup with a harsher syntax. Dropped by preference. |
| **WiX** | The sane way to author an MSI, but a third party, and WiX v6+ carries an Open Source Maintenance Fee — free at zero revenue, a live question for commercial reuse. If MSI wins, it wins with the SDK. |
| **MSIX** | Virtualises `%APPDATA%` inside a container. It would fight the scheduled task and the configuration file. |

`winget` is a channel, not a format; it can point at whichever wins.

**The trap that survives whichever is chosen:** the scheduled task records an
absolute path. An upgrade that relocates the executable must re-run
`install-task`, or the logon task silently points at a file that no longer
exists.

## Updating, and what it does to the format choice

If a release is detected and a menu entry applies it, what becomes of an
MSI's install state once a home-made updater has replaced its files?

**What Windows Installer actually does.** Nothing at the time: it watches no
files. It keeps a registration — components, key paths, the product version —
and a cached copy of the package, and consults the disk at four moments:

| Moment | With a newer file put there by an updater |
| --- | --- |
| Self-repair | Fires only through an *advertised* entry point — shortcut, COM class, extension — and checks that the key-path file **exists**, not its version. A newer file: nothing. A key-path file the updater *removed*: repaired from the cache, so the old version comes back. This package can have no advertised entry point at all; the watcher starts from the logon task. |
| Explicit repair (`msiexec /f`, the ARP button) | Default mode `omus`: reinstall if missing **or older**. A newer *versioned* file is left alone — which requires a `VERSIONINFO` resource; for unversioned files MSI falls back to hash and date rules that are exactly the swamp to avoid. |
| Uninstall | Removes each component's file at its path whatever its version. A file the updater *added* that the package never knew is orphaned. |
| The next MSI (major upgrade) | With `RemoveExistingProducts` sequenced **early**, after `InstallInitialize`, the old product is removed and every new file copied. Sequenced late, the newer-on-disk rule skips the copy and the old product's removal then deletes the file — the classic "file vanished after upgrade". |

And a permanent effect: Add/Remove Programs shows the MSI's `ProductVersion`,
not the executable's. A file-swapping updater makes the panel lie — the
documented symptom of Chrome's enterprise MSI.

**Verdict.** MSI does not prevent a home-made updater, but an updater that
swaps files under *any* installer is the wrong shape. The right shape, for
either format, is that **the installer is the updater**: detect, download,
verify, run the new installer silently, restart. One owner of the installation.
Per-user MSI: `msiexec /i new.msi /passive`, no UAC. Inno:
`setup.exe /SILENT /SUPPRESSMSGBOXES` — VS Code's model exactly.

The **zip** is the opposite case: nothing else owns the files, so there the
updater swaps them itself. `MsiEnumRelatedProducts` on the package's
UpgradeCode tells the two apart.

The updater itself — the menu entry, the check against GitHub, the
download, the relaunch — is [Lot 13](13-updating.md), decided on 2026-09-17
to be a lot of its own. What stays here is what it demands of the package.

**Does it decide Inno against MSI?** No. Both need the same updater. It adds
two cheap requirements to MSI — `VERSIONINFO`, wanted anyway, and the early
`RemoveExistingProducts` — and none to Inno; it puts a thumb on Inno's side for
the silent run and `CloseApplications`. The choice stays with the lot.

## Code signing

Considered and declined for now. A certificate from a private CA is not
trusted by anyone else's Windows, so it buys nothing against SmartScreen; a
publicly trusted one requires an HSM since 2023 and Azure Artifact Signing is
limited to individuals in the US and Canada. Sigstore provides provenance —
"this artifact came from this workflow run" — not Authenticode, and Windows
does not check it. Revisit if the program reaches an audience for whom
SmartScreen warnings matter.
