# Lot 8 — Distribution

**Status: proposed.** Partly done: `scripts/build.ps1` runs the whole checklist
and produces the portable bundle in `dist/`, and `.vscode/tasks.json` drives
it. The script was written first on purpose — a release that cannot be made by
hand is not one CI can make either. What is left needs a public repository:

- Create the public GitHub repository and push — **with explicit approval**
- A release workflow on a `v*` tag that runs the same script and attaches its output
- An installer, per-user, into `%LOCALAPPDATA%\Programs\GameModeExecutor`
- `VERSIONINFO` metadata in the executables, through the same `rc.exe` step that embeds the icon
- The install section of the documentation pointing at a release rather than at `cargo build`

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

### Two candidates, decided when the lot is taken

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

Both candidates can offer per-user *or* per-machine from one installer, but
**this program has no per-machine story**: the logon task, the configuration
and the log are all per-user. A per-machine install would still leave every
user to run `install-task`, and charge an elevation prompt for the privilege.
Per-user only — in Inno Setup that is `PrivilegesRequired=lowest` and
`DefaultDirName={userpf}\GameModeExecutor`.

Ruled out, so they are not reconsidered from scratch:

| | |
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

The **portable** bundle is the opposite case: nothing else owns the files, so
there the updater swaps them itself. `MsiEnumRelatedProducts` on the package's
UpgradeCode tells the two apart.

Two things this settles for the updater, whenever it comes:

- *The running executable.* The watcher holds its own `.exe`; MSI's Restart
  Manager would show a files-in-use dialog even under `/passive`. So: refuse to
  update while a game is on, then launch the installer *and quit*, with
  `msiexec … & schtasks /Run Watcher` in a detached `cmd` to relaunch — no
  custom action.
- *The "no network" non-goal.* An automatic release check breaks it. The
  compatible shape is a **Check for updates…** entry that connects only when
  clicked, or an explicit opt-in — never a silent poll. Over **WinHTTP**, a
  Microsoft library using the system certificate store. The download verified
  against a SHA-256 published with the release, which guards against
  corruption and not against a compromised account.

**GitHub allows and provides for the check, with no key.** Either the REST
API — `GET /repos/{owner}/{repo}/releases/latest`, 60 requests an hour per IP
unauthenticated, `User-Agent` mandatory — or no API at all:
`github.com/{owner}/{repo}/releases/latest` answers 302 with the tag in
`Location`, and `…/releases/latest/download/{asset}` serves the latest asset,
via a redirect that changes host to `objects.githubusercontent.com`. The second
suffices: a `HEAD`, a `Location`, a tag compared with `build_info::VERSION`,
nothing to parse. A token would only matter for a private repository, and
embedding one in a public executable would be a fault.

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
