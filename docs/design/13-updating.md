# Lot 13 — Updating

**Status: proposed, measured against a real release on 2026-09-18.** Decided
2026-09-17 to be a lot of its own rather than a tail of
[Lot 8](08-distribution.md): updating touches the "no network" non-goal, the
tray menu and the running process, and each of those deserves its own
measurement. Nothing here is built. Lot 8 is done and `v0.1.0` exists, so
there is now something to update from, and what a release actually answers
is recorded below rather than assumed.

**Goal.** A user who wants the newer version gets it from the notification
icon, without a browser, without an administrator prompt, and without the
program ever connecting on its own.

**Done when:** *Check for updates…* in the menu finds the latest release,
says what it found, installs it on request while no game is running, and
the watcher comes back on the new version — verified in the field across a
real release pair.

## What Lot 8 settled, and what it then did for this lot

**The installer is the updater.** An updater that swaps files under an
installer is the wrong shape, for reasons the distribution page keeps in
its table of Windows Installer's four moments. So the updater downloads the
new package, verifies it, and runs it silently: `msiexec /i new.msi`,
per-user, no UAC. `MsiEnumRelatedProducts` on the package's UpgradeCode —
already in `purge` — tells an installed copy from an unpacked one.

**The package now stops and restarts the watcher itself.** This page first
proposed that the watcher quit before launching the installer and hand its
own relaunch to a detached shell, because Windows Installer's Restart
Manager would otherwise put up a files-in-use dialog. Lot 8 met that dialog
on its first uninstall and answered it in the package: an immediate action
runs `stop` before `InstallValidate`, and `install-task` at the end starts
the watcher through its task. Measured on a real upgrade on 2026-09-18:
700 ms from *Stopped* to *starting*, no dialog, one product listed. So the
updater has less to do than planned — start the installer detached and let
the package close the process that started it; the new version comes back
by the package's own doing. What the updater still owns is the failure
path: if the install fails after the watcher was stopped, nothing restarts
it until the next logon, so something must wait for `msiexec` and run the
task again when it exits non-zero. The same idiom as `purge`'s after-exit
shell: hidden Windows PowerShell, `Wait-Process`, then `schtasks /Run`.

## What the release answers, measured 2026-09-18 against `v0.1.0`

| Request | Answer |
| --- | --- |
| `HEAD github.com/{owner}/{repo}/releases/latest` | `302`, `Location: …/releases/tag/v0.1.0` — the tag, and nothing to parse but a URL |
| `HEAD …/releases/latest/download/SHA256SUMS.txt` | `302` to `…/releases/download/v0.1.0/SHA256SUMS.txt`, then the file: one line per asset, `<sha256>  <name>` |
| `…/releases/download/v0.1.0/GameModeExecutor-0.1.0.msi` | `302` to a signed `release-assets.githubusercontent.com` URL valid for about an hour, then `200`, `Content-Length: 1462272`, `application/octet-stream` |
| `GET api.github.com/repos/{owner}/{repo}/releases/latest` | JSON: `tag_name`, `name`, `draft`, `prerelease`, `published_at`, `body`, and per asset `name`, `size`, `browser_download_url` and `digest: sha256:…`; `X-RateLimit-Limit: 60` per IP unauthenticated, `Cache-Control: max-age=60`, an `ETag` |

Two witnesses to the same hash: the workflow's `SHA256SUMS.txt`, computed
on the runner from the files it built, and GitHub's own `digest` on each
asset, computed on upload. For the `.msi` they agreed,
`85d6178b…d0d5d1`. Neither defends against a compromised account, only
against a corrupted or truncated download, and the record says so.

**What the program already knows without connecting:** its own version,
`build_info::VERSION`; whether Windows Installer owns it and under which
product code, `purge::installed_product()`; the installed product's
version, `MsiGetProductInfoW` with `VersionString` — which is the number
*Programs and Features* shows and the one an upgrade must beat; whether a
game is running; whether the logon task exists.

## The shape, decided ahead of building it

- **Never a silent poll.** An automatic release check breaks the "no
  network" non-goal. *Check for updates…* connects when clicked and at no
  other time. An opt-in check at start is not offered in this lot; if it
  ever is, it is a configuration key that defaults to off, at most once a
  day, and the record says what it sends.
- **The check is one request and no API.** `HEAD …/releases/latest` with
  redirects disabled, the tag read from `Location`, `x.y.z` parsed from it
  and compared with the running version as three numbers. A tag that does
  not parse as exactly `vX.Y.Z` is "a release this version does not
  understand", shown as such, never guessed at. No rate limit to think
  about, no JSON, no `User-Agent` contract. The API stays in reserve for
  the release notes, should the menu ever show them.
- **Every later request names the tag, not `latest`.** The checksum file
  and the package are fetched from `…/releases/download/<tag>/…`, so a
  release published between the check and the download cannot mix one
  version's hash with another's file.
- **Verified against `SHA256SUMS.txt`**, the line for the package's exact
  file name, with the hash computed through BCrypt — a Microsoft library,
  no crate. A mismatch deletes the file and says so; nothing is ever run
  unverified.
- **Over WinHTTP**, a Microsoft library using the system certificate store
  and the system proxy (`WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY`). No HTTP
  crate, no relaxed certificate flag. It follows `https` → `https`
  redirects by default, which the asset chain needs, and can be told not
  to for the one request whose redirect *is* the answer.
- **Downloaded to `%LOCALAPPDATA%\GameModeExecutor\updates\`**, local and
  disposable like the log; the watcher empties that folder when it starts,
  so a package is kept only until the version it carries is running.
  Windows Installer caches its own copy of every package it installs, so
  deleting the download costs a later repair nothing.
- **Installed with `msiexec /i <file> /qn /l*v <updates>\install.log`**,
  started detached — not a child that shares the watcher's fate — and the
  package's own actions stop this process and start the new one. `/qn`
  rather than `/passive`, provisionally: `/passive` shows Windows
  Installer's progress window, and the program's rule is no windows; the
  icon going and coming back is the visible part, as it is for the
  installer run by hand, and the version in the tooltip afterwards is the
  confirmation. To be measured on screen before it is settled.
- **Refused while a game is running**, for the purge's reason: the stop
  commands would fire mid-game and the new watcher would fire the start
  commands seconds later. The menu says so; the user quits the game and
  clicks again.
- **The zip copy is told, not updated.** Nothing owns its files but the
  user, and replacing two executables under a running logon task from a
  hidden shell is exactly the file-swapping shape this lot exists to avoid.
  A hand-installed copy gets the same check, and the menu entry then
  opens the release page. The person who chose no installer keeps their
  files in their hands.
- **The menu is the whole interface**, as everywhere else: the entry reads
  *Check for updates…*, then *Up to date (0.1.0)* greyed, or *Update to
  0.2.0…*, or *Could not check: offline* greyed; the tooltip mirrors it.
  No balloon, no dialog. The log carries the same lines under `setup`,
  with the URL, the size and the hash, so an update is as readable
  afterwards as an install.
- **A downgrade is never offered.** The package refuses one anyway
  (`NEWERVERSIONDETECTED`), and the comparison makes it unreachable.

## What it does not defend against, said plainly

- **A compromised release or account.** The hash proves the file is the one
  the workflow published, not that the workflow was honest. Authenticode
  was declined in Lot 8 for want of a certificate anyone else's Windows
  trusts; that decision, not this lot, is where the line moves if it ever
  does.
- **SmartScreen does not see it.** A file fetched through WinHTTP carries
  no Mark of the Web — browsers and Explorer write it, libraries do not —
  so `msiexec` runs the package without the warning a person downloading
  the same file would meet. Convenient, and worth knowing: the program is
  the one vouching for the file, through the hash and nothing else.
- **A stale mirror or a captive portal.** A `302` to somewhere that is not
  GitHub, or a `200` that is an HTML page, must fail the parse and be
  shown as "could not check", never as "up to date".

## To measure, when the lot is taken

1. WinHTTP against the four requests above: reading `Location` with
   redirects disabled, following the asset chain to the signed URL with
   them enabled, a proxy, an offline machine and a DNS failure, each as
   seen from the menu and the log.
2. The watcher launching its own upgrade: `msiexec /qn` detached,
   `StopWatcher` closing the process that started it, `RegisterTask`
   bringing the new version back — and the failure path, with a package
   built to fail after `InstallValidate`, restarting the old one.
3. `/qn` against `/passive`, on screen, success and failure.
4. The updates folder emptied at start while Windows Installer's cache
   still serves a repair.
5. The zip path: the check, the notice, the page opening, and nothing else
   happening.

## Size

A module of a few hundred lines — the requests, the hash, the version
comparison, the launch — with the comparison and the `SHA256SUMS.txt` parse
under unit tests, one menu entry and one tooltip state in the tray, and a
`setup` line for each step. Verifying it needs a real release pair: it is
built against `v0.1.0` and proved by installing whatever `v0.1.1` becomes.
