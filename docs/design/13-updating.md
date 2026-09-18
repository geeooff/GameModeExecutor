# Lot 13 — Updating

**Status: in progress since 2026-09-18.** Decided 2026-09-17 to be a lot of
its own rather than a tail of [Lot 8](08-distribution.md): updating touches
the "no network" non-goal, the tray menu and the running process, and each
of those deserves its own measurement. The shape below was agreed with the
maintainer on 2026-09-18, against `v0.1.0`, before a line was written.

- [x] The session handed from one watcher to the next: `stop --handover`, and a start that resumes an open session instead of closing it — three scenarios in `engine/tests.rs`, 2026-09-18
- [x] `update`: the state machine, tested whole through a scripted feed; the WinHTTP feed and the CNG hash behind it — 2026-09-18, and no test ever calls GitHub: the network path is measured by hand, below
- [x] The menu section, rendered from the machine and nothing else — 2026-09-18, to be seen on screen
- [x] The package: `StopForUpgrade` hands over, `StopForRemoval` restores — 2026-09-18
- [x] The zip copy updates itself the same way, through the after-exit shell — 2026-09-18, the script tested for its shape
- [x] The documentation: *Getting started*, *How it works*, the reference, the README's word on the network — 2026-09-18
- [x] Measured on the maintainer's machine, 2026-09-18 13:03–13:33, both paths against the real `v0.1.0`: the handover mid-game and the resume, the check, the download and its verification, the install from the zip and from the package, the watcher back on the new version — below
- [ ] Measured: offline and behind a proxy, as seen from the menu; the failure path restarting the old watcher; `/qn` on screen
- [ ] Verified in the field across a real release pair

**Goal.** A user who wants the newer version gets it from the notification
icon, without a browser, without an administrator prompt, and without the
program ever connecting on its own.

**Done when:** *Check for updates* in the menu finds the latest release,
says what it found, installs it on request — mid-game included — and the
watcher comes back on the new version with the game session intact,
verified in the field across a real release pair.

## What Lot 8 settled, and what it then did for this lot

**The installer is the updater.** An updater that swaps files under an
installer is the wrong shape, for reasons the distribution page keeps in
its table of Windows Installer's four moments. So for an installed copy the
updater downloads the new package, verifies it, and runs it silently:
`msiexec /i new.msi /qn`, per-user, no UAC. `MsiEnumRelatedProducts` on the
package's UpgradeCode — already in `purge` — tells an installed copy from an
unpacked one.

**The package stops and restarts the watcher itself.** Lot 8 met the Restart
Manager's dialog on its first uninstall and answered it in the package: an
immediate action runs `stop` before `InstallValidate`, and `install-task`
at the end starts the watcher through its task. Measured on a real upgrade
on 2026-09-18: 700 ms from *Stopped* to *starting*, no dialog, one product
listed. So the updater starts the installer detached and lets the package
close the process that started it; the new version comes back by the
package's own doing. What the updater still owns is the failure path: if
the install fails after the watcher was stopped, nothing restarts it until
the next logon, so a hidden shell waits for `msiexec` and runs the task
again when it exits non-zero — `purge`'s after-exit idiom.

**The zip copy is not told, it is updated** — decided 2026-09-18 after this
page had, for a night, proposed a notice and a link instead. The shape to
avoid is swapping files under an installer that owns them; nothing owns an
unpacked copy's files but the user, which is what the distribution page
had said all along. The same hidden shell does the work in that mode: wait
for the watcher to exit, expand the archive over the folder — the zip ships
no `config.toml`, so a configuration beside the executables is untouched —
keep the previous executables as `.old` until the new version has started,
then `install-task`. Not transactional, unlike Windows Installer, and the
page says so; the old files stay until the new ones are in place.

**The commands are the contract, and each caller sequences them.** The
package, the zip's shell and `purge` each call `stop`, `init`,
`install-task` and `uninstall-task` in the order their own mechanism
allows; a shared "finish" verb was considered on 2026-09-18 and declined,
because the package could not call it at the moments Windows Installer
dictates anyway and it would exist for symmetry alone. The cost of that
freedom is a rule, in `AGENTS.md`: a change to any of those commands is
verified on all three paths.

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
product code, `package::installed_product()`; whether a game is running;
whether the logon task exists.

## The session is handed over, not closed

Decided 2026-09-18, when the maintainer asked why an update should wait for
the game to end. The first draft of this page refused to install mid-game,
because `stop` is *Quit*: the stop commands fire, the marker goes, and the
new watcher then detects the running game and fires the start commands —
a two-second blip of the idle configuration in the middle of a session.
The refusal avoided the blip from the wrong end. The session marker
already says "a game session is open"; what was missing was a stop that
leaves it saying so.

| | *Quit*, `stop` | `stop --handover` |
| --- | --- | --- |
| Who follows | nobody | a watcher, within the second |
| Stop commands | run now — the standing promise, "not left on a gaming configuration" | **do not run** |
| Marker | removed once the commands are confirmed | **left in place**: the session is open, and handed on |
| Log | `Stopping while a game is running, so the stop commands run now` | `Stopping for an update; the game session is handed to the next watcher` |

And at start, `recover()` learns the distinction [Lot 9](09-robustness.md)
had already written down for the configuration-fault case — *look for the
writer before recovering*:

- marker present **and the presence writer alive** → **resume**: the name
  from the marker, the icon active, the wait on the writer's handle taken
  up again. Nothing runs, neither stop nor start: the start already
  happened. No refinement either; the name in the marker is the refined
  one when there was one.
- marker present, writer gone → the recovery of today: the stop commands
  run, because the game ended in the gap, or during a logoff.

It covers more than the updater: a watcher that crashes mid-game and is
restarted by its task, the failure path restarting the old watcher, the
development loop of `stop` then `install-task` — all resume where they
used to blip. `StopSignal` carries a reason, `Restore` or `Handover`; the
session window takes an application message beside `WM_CLOSE` so a
`stop --handover` from another process can say which; the engine
branches on it. Three scenarios in `engine/tests.rs`: a handover mid-game
runs nothing and leaves the session open, a handed-over session is
resumed by the next watcher without running anything, and a handed-over
session whose game ended in between runs the stop commands at start.

**In the package**, two stop actions where there was one: `StopForUpgrade`,
`stop --handover`, conditioned on `PREVIOUSVERSIONS` — a successor is
guaranteed by `RegisterTask` in the same sequence — and `StopForRemoval`,
plain `stop`, on an uninstall, where nobody follows and the machine must
be restored. **One degradation, accepted on 2026-09-18:** the first package
to carry `--handover` runs it on the installed `0.1.0`, which does not know
the flag; the action fails, continues, and the Restart Manager's dialog
comes back for that one upgrade on the two machines that have `0.1.0`.
Clicking through was measured clean on 2026-09-18 01:08. A legacy action
kept forever for two machines was not worth it.

`purge` keeps refusing mid-game: nobody follows a purge.

## The machine, and the menu that renders it

**The UI reflects an object.** Every rule lives in `update`: which entries
exist in which phase, which actions are legal, when a verdict expires. The
tray asks `view()` for a list of items and calls `perform(action)` for the
one chosen; it holds no rule of its own. The object is driven by events,
so it is tested whole without a network, through the same seam the engine
uses for the OS: a `Feed` trait — the latest tag, a text file, a download —
with `WinHttp` as the one real implementation and a scripted one for the
tests.

```
Idle                                   nothing to say
Checking                               one request in flight
UpToDate    { version, at }            a verdict about now
Available   { release }                tag, version, page, file name, hash
Downloading { release, size }
Installing  { release }                msiexec, or the zip's shell, is running
Failed      { fault, during, at }      a sentence, a code, the log has the rest
```

`apply(event, now)` takes `CheckAsked`, `CheckDone(Ok(verdict) | Err(fault))`,
`InstallAsked`, `DownloadStarted { size }`, `DownloadDone(Ok | Err)`,
`InstallFailed(fault)` and `FoundAtStart(fault)`, and hands back the
`Effect` the worker must go and run — `Check`, `Download(release)`,
`Install(release)` — or nothing, for an answer nobody asked for or a click
the phase does not take. `view(now) -> Vec<Item>`, an `Item` being a label,
an optional `Action` — `Check`, `Install`, `OpenReleasePage(url)` — and
whether it is enabled. Expiry is computed in `view` from the phase's `at`;
there is no timer. `Failed` with no `at` is a failure found at start, kept
until the next check.

The section sits between *Documentation*'s separator and *Quit*:

| Phase | Entries (⊘ disabled) |
| --- | --- |
| Idle | Check for updates |
| Checking | ⊘ Checking for updates… |
| UpToDate, within the hour | Check for updates · ⊘ 0.1.0 is the latest version |
| Available | Check for updates · **Download and install 0.2.0** · What changed in 0.2.0 |
| Downloading | ⊘ Check for updates · ⊘ Downloading 0.2.0 (1.4 MB)… · What changed in 0.2.0 |
| Installing | ⊘ Check for updates · ⊘ Installing 0.2.0… |
| Failed, within the hour | Check for updates · ⊘ Could not check: no connection (see log) |
| A failure found at start | Check for updates · ⊘ Update to 0.2.0 failed: Windows Installer 1603 (see log) |

Three entries at most, never two disabled ones outside a download. *Check
for updates* is clickable again as soon as a result exists and clears the
rest; it is disabled while a download or an install is running, where a
new check would mean nothing. The tooltip and the icon do not change: the
updater says nothing through the icon.

**The menu closes on the click, and the answer is a notification.** Seen
on the first field run: choosing *Check for updates* closes the menu, as
choosing anything in a Windows menu does, and the verdict then waits in a
menu nobody has reopened. Keeping a popup menu open through a click has
no supported path — `TrackPopupMenuEx` returns when the choice is made —
and closing one after a delay would be a menu doing what no other menu
does. So the outcome of what the user clicked reaches them the way
Windows reports finished background work: a notification from the icon
(`NIF_INFO`), silent (`NIIF_NOSOUND`), held back during quiet hours,
shown as a toast and kept in the notification centre. *Up to date*,
*Update available — right-click the icon to download and install it*,
*Installing 0.2.0*, or the fault and *See the log* — and, from the new
version at its first start, *Updated to 0.2.0*: on the maintainer's
machine the install went by in a second, too quick to see the version
change, so the version that came out of it says so (2026-09-18). Only
ever to answer a click, never for anything the program did on its own;
and the same answer stays in the menu. The object owns it: `Machine` leaves a `Notice`
on each outcome, the worker wakes the window's thread through a callback
the tray handed in, and the tray takes the notice and draws it — no rule
in the tray. The earlier line of this page, *no balloon*, was written
before a person had clicked; corrected 2026-09-18.

**Two expiries, not one.** `UpToDate` and `Failed` are claims about *now*
and expire after an hour. `Available` does not expire: a release does not
un-release, and someone who said "later" should find the offer where they
left it rather than click twice. Both were the maintainer's call on
2026-09-18, between five minutes and a day.

**The check is one request and no API.** `HEAD …/releases/latest` with
redirects disabled, the tag read from `Location`, `x.y.z` parsed from it
and compared with the running version as three numbers. A tag that does
not parse as exactly `vX.Y.Z` is "a release this version does not
understand", shown as such, never guessed at. Every later request names
the tag, not `latest`, so a release published between the check and the
download cannot mix one version's hash with another's file. The API stays
in reserve for the release notes, should the menu ever show them; *What
changed* opens the release page in the browser.

**Never a silent poll.** *Check for updates* connects when clicked and at
no other time. An opt-in check at start is not offered; if it ever is, it
is a configuration key that defaults to off, at most once a day, and the
record says what it sends. **A downgrade is never offered**; the package
refuses one anyway.

**The same from the console.** `gamemode-executor update` drives the same
object — `--check` prints the verdict and stops, the default downloads,
verifies and installs — so a script, a diagnosis or the second machine's
maintainer can do what the menu does, and the network path can be
measured from a shell without a watcher.

**Measured through the code on 2026-09-18**, with `update --check` from a
console and a run of the feed by hand, against `v0.1.0`: the `HEAD` answers
`302` with the tag in 330 ms; the checksum file comes through its redirect;
the 1.4 MB installer comes through the signed-URL chain in 340 ms and
hashes to the release's line. One thing the record could not have known:
closing a WinHTTP session cancels every request under it, and the first
body read failed with `12017` until the session and connection handles
were kept alive with the response. No test calls GitHub — the script runs
the ignored tests on every developer machine — so this is a hand
measurement, repeated with the command whenever the feed changes.

**What the tests do instead, decided 2026-09-18:** a listener of their
own on `127.0.0.1`, written from the standard library in the test module
of `winhttp.rs`, that answers the record's table — the `302` with the tag,
a redirect to a *second* listener for the asset host, a body larger than
one read, a cut-off download, a `404`, a `503`, a page where a text file
should be, a port nobody listens on. It talks plain `http` to a
`#[cfg(test)]` constructor of the feed that the shipped program does not
have; a TLS server without a crate would be SChannel by hand, and TLS is
WinHTTP's, not ours. The `12017` defect above is the kind this catches.
A local certificate was considered and declined: absent from the runner,
and a test that installs one touches the machine's trust store. So was
HTTP.sys, which IIS and .NET's `HttpListener` serve HTTPS through, with SNI
bindings since Windows 8: binding a certificate to a port is `netsh http
add sslcert`, administrator only, and a non-administrator cannot reserve a
URL prefix for a listener without `netsh http add urlacl` either — a test
can do neither unelevated, and one that could would be changing the
machine. Considered on the maintainer's remark, 2026-09-18. What TLS
does when the certificate is wrong was measured by hand instead, the same
day, against `expired`, `self-signed` and `wrong.host` at badssl.com:
`NoConnection { code: 12175 }`, `ERROR_WINHTTP_SECURE_FAILURE`, all three
— the program relaxes no flag. A comparison test against GitHub, to
measure drift, was declined for the same rule; drift shows up as
"unexpected answer" from `update --check`, with the headers at `debug`,
and the record and the listener are corrected together.

**A stand-in for GitHub as a project of its own** — a Python or .NET
minimal API beside the repository, for end-to-end runs — was weighed on
2026-09-18 and declined. It would not buy TLS either: the blocker is the
client, which trusts only what the machine trusts, and a development
certificate is trusted through a consent prompt or the root store, neither
of which a runner has. It would buy fidelity the code does not read,
against a process to start and stop, a second toolchain, and a dependency
tree of its own to keep patched. Should an end-to-end run against the real
binary ever be worth having, the honest shape is not a fake GitHub but a
real one: a repository of test releases, with real HTTPS, real redirects
and real signed URLs, reached through a configuration key naming the
repository to update from — a key a fork would need anyway, and one that
opens nothing new, since whoever can edit the configuration can already
name any executable in it. Run by hand or on a `workflow_dispatch`, never
in `test`, because it calls an external host. Kept in reserve until a fork
asks for it.

## What the first field run taught, 2026-09-18

Both paths, the same afternoon, against the real `v0.1.0`, from a `0.0.9`
build of the branch. The zip first, with the package uninstalled so the
task pointed at the unpacked copy; then the package.

- **The handover works in the field, mid-game, on both.** Starfield on,
  `stop --handover`, `install-task`: *Stopping for an update; the game
  session is handed to the next watcher*, then twenty seconds later *The
  last watcher left a session open with Starfield.exe still running, so it
  resumes where it was* — no command ran, the fans stayed where they were,
  the icon came back green with the name.
- **The zip path**: *Update available: 0.1.0* with the zip's hash, 1.7 MB
  downloaded and verified, *Installing*, the watcher stopped itself, and
  one second later *GameModeExecutor 0.1.0 starting* — from the same folder,
  through the task kept as it was. The archive was expanded over the folder
  with the executables kept as `.old`.
- **The package path**: the same lines with the installer's hash and
  1.5 MB, then the package's own actions: *Watcher stopped, as asked* —
  plain, since `0.1.0`'s package does not know `--handover` — configuration
  kept, task kept, the new watcher up 0.9 s after the download.
- **A false failure, and a better verdict.** The `0.1.0` installed does not
  know `pending.txt`, so it never consumed it; the `0.0.9` package installed
  next read *0.1.0 pending* and reported the update failed. It had not: the
  installer's own log ended with *Installation success or error status:
  0*. `settle` now reads that verdict — UTF-16 with a byte-order mark, as
  `msiexec /l*v` writes it — and says *installed, and this is 0.0.9 by other
  means* at `info`, or names the installer's error code when there is one,
  before falling back to *did not take*.
- **The menu closed on the click** — above.
- **A watcher started by the package took itself for an unpacked copy**,
  17:44 the same day, on the second run of the package path: it checked,
  found the *zip*, expanded it over the package's own folder, and left a
  product registered as 0.0.9 with 0.1.0 files, `docs\` and `.old`
  executables beside them. The kind of copy was decided once, at start —
  and the package starts the watcher from `RegisterTask`, sequenced
  *before* `RegisterProduct`: at that instant Windows Installer knows no
  product, and the folder rule alone says unpacked. The first run of the
  path had passed by luck, on a watcher restarted by hand after the
  install. The kind is now decided when the question is asked, from what
  Windows Installer says at that moment; the tests pin it. Cleaned by
  uninstalling the package and deleting what it did not own.
- **A word swallowed in the resume line**: the source carried a run of
  spaces where a line continuation had been meant, and the log showed it.
  Fixed; the pitfall was the editing tool, not the code.

## Faults, and what the log says

A network is an outside dependency the program did not have before, so
every step is written down, under a new category, `update` —
`RUST_LOG=update=debug` isolates everything that touches it:

| Cause | Menu | Log |
| --- | --- | --- |
| DNS, connection refused, timeout | Could not check: no connection (see log) | `warn`, the WinHTTP code as a field |
| GitHub answered 4xx/5xx, or 429 | Could not check: GitHub answered 503 (see log) | `warn` |
| An unexpected answer — no `Location`, a tag that is not `vX.Y.Z`, HTML where the checksums should be, a captive portal | Could not check: unexpected answer (see log) | `warn`, the headers at `debug` |
| The hash does not match | Download failed: the file did not verify (see log) | `warn`, the file deleted |
| Disk full, folder not writable | Download failed: cannot write to …\updates (see log) | `warn`, the Win32 code |
| `msiexec` refuses before stopping the watcher — 1618 another install running, 1638 | Update failed: Windows Installer 1618 (see log) | `warn`; the watcher is still there to say so |
| A failure *after* the watcher stopped — 1603, the zip's extraction, `install-task` | seen at the next start: Update to 0.2.0 failed: … (see log) | the shell restarts the previous watcher and writes `updates\result.txt`; the watcher reads it at start, logs `warn`, shows it until the next check |

At `info`, the story: `Checking for updates` · `0.1.0 is the latest` ·
`Update available: 0.2.0` · `Downloading 0.2.0 (1.4 MB)` · `Downloaded and
verified 0.2.0` · `Installing 0.2.0; the watcher stops now and comes back
on the new version` — then, from the new watcher, `Updated to 0.2.0`. The
watcher writes `updates\pending.txt` with the version it is installing
before it launches anything; whichever watcher starts next compares that
file with its own version — equal, the update took; different, it did
not, and `result.txt` says why when the shell got as far as writing it.
Every menu line that ends in *(see log)* means it.

Downloads go to `%LOCALAPPDATA%\GameModeExecutor\updates\`, local and
disposable like the log; the watcher empties it at start once the pending
file has been read. Windows Installer caches its own copy of every package
it installs, so deleting the download costs a later repair nothing.

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

## To measure, when the pieces exist

1. WinHTTP against the four requests above: reading `Location` with
   redirects disabled, following the asset chain to the signed URL with
   them enabled, the system proxy, an offline machine and a DNS failure,
   each as seen from the menu and the log.
2. A self-launched upgrade mid-game: `msiexec /qn` detached,
   `StopForUpgrade` handing the session over, `RegisterTask` bringing the
   new version back, the session resumed with nothing run — and the
   failure path, with a package built to fail after `InstallValidate`,
   restarting the old one, which resumes too.
3. `/qn` on screen, success and failure.
4. The updates folder emptied at start while Windows Installer's cache
   still serves a repair.
5. The zip path, on an unpacked copy: the files replaced under the logon
   task, `.old` kept until the new version starts, and what happens when
   the task fires in the middle of it.
