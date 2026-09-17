# Lot 13 — Updating

**Status: proposed.** Decided 2026-09-17 to be a lot of its own rather than
a tail of [Lot 8](08-distribution.md): updating touches the "no network"
non-goal, the tray menu and the running process, and each of those deserves
its own measurement. Nothing here is built. It needs Lot 8 first — there is
nothing to update to until a release exists.

**Goal.** A user who wants the newer version gets it from the notification
icon, without a browser, without an administrator prompt, and without the
program ever connecting on its own.

**Done when:** *Check for updates…* in the menu finds the latest release,
says what it found, installs it on request while no game is running, and
the watcher comes back on the new version — verified in the field across a
real release pair.

## What Lot 8 already settled

**The installer is the updater.** An updater that swaps files under an
installer is the wrong shape, for reasons the distribution page keeps in
its table of Windows Installer's four moments. So the updater downloads the
new package, verifies it, and runs it silently: `msiexec /i new.msi
/passive`, per-user, no UAC. The **zip** is the opposite case — nothing else
owns the files, so there the updater replaces them itself.
`MsiEnumRelatedProducts` on the package's UpgradeCode tells the two
installations apart.

## What is decided, ahead of building it

- **Never a silent poll.** An automatic release check breaks the "no
  network" non-goal. The compatible shape is a *Check for updates…* entry
  that connects only when clicked, or an explicit opt-in in the
  configuration; nothing else ever opens a connection.
- **Over WinHTTP**, a Microsoft library using the system certificate store.
  No HTTP crate.
- **The download verified against a SHA-256 published with the release.**
  That guards against corruption, not against a compromised account, and
  the record says so.
- **GitHub provides for the check with no key.** Either the REST API —
  `GET /repos/{owner}/{repo}/releases/latest`, 60 requests an hour per IP
  unauthenticated, `User-Agent` mandatory — or no API at all:
  `github.com/{owner}/{repo}/releases/latest` answers 302 with the tag in
  `Location`, and `…/releases/latest/download/{asset}` serves the latest
  asset through a redirect to `objects.githubusercontent.com`. The second
  suffices: a `HEAD`, a `Location`, a tag compared with
  `build_info::VERSION`, nothing to parse. A token would matter only for a
  private repository, and embedding one in a public executable would be a
  fault.
- **The running executable.** The watcher holds its own `.exe`; Windows
  Installer's Restart Manager would show a files-in-use dialog even under
  `/passive`. So: refuse to update while a game is on — the same rule as the
  purge — then launch the installer *and quit*, with the relaunch handed to
  a detached shell (`msiexec … & schtasks /Run Watcher`). No custom action.

## To measure, when the lot is taken

1. The redirect chain of `releases/latest` and `releases/latest/download`
   through WinHTTP, and what a rate-limited or offline answer looks like
   from the menu.
2. A `/passive` upgrade launched by the watcher itself, with the watcher
   gone by the time Windows Installer looks for files in use.
3. The zip path: replacing two executables under a running logon task, and
   what happens when the task fires in the middle of it.
