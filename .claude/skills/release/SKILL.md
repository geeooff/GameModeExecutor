---
name: release
description: Prepare and publish a GameModeExecutor release (vX.Y.Z), from the checks before the release commit to the field checks after it. Use when the maintainer asks to prepare, cut or publish a release.
---

# Releasing GameModeExecutor

Every push, merge and tag below needs the maintainer's explicit approval, each
time.

## 1. Ready?

- `main` holds every lot meant for the release; nothing left on a branch.
- `CHANGELOG.md`'s `[Unreleased]` lists what the user sees, one short line per
  change (the rules are in `AGENTS.md`).
- The user pages are short and current: README, *Getting started*, *How it
  works*, the reference, the recipes. The design index's *Today* column says
  where each lot stands.
- If the idle look or the in-game wait changed since the last release, measure
  the installed watcher against it (`presence-probe cost`, `footprint`,
  `menu-cost`) and put the figures in the record and the changelog.
- External links, by hand, FanControl's site and releases first —
  `build.ps1` checks only the repository's own links and sections:

  ```bash
  grep -rhoE 'https?://[^ )>`"]+' README.md CHANGELOG.md docs | sed 's/[.,;]$//' | sort -u
  ```

  then `curl -s -o /dev/null -L -w '%{http_code}'` each; expect 200. A URL
  ending in `)` is cut by the pattern: check it whole.
- The version, per `docs/design/08-distribution.md`: patch for fixes; minor
  for a feature or, while 0.x, a contract change said in the notes. 1.0.0
  waits for its criteria.

## 2. The release commit

On `release/X.Y.Z`, branched from `main`:

1. `version = "X.Y.Z"` in `Cargo.toml`; a build updates `Cargo.lock`.
2. `CHANGELOG.md`: `## [Unreleased]` becomes `## [X.Y.Z] - YYYY-MM-DD`, the
   commit's date, with a one-sentence summary of the release under it; a new,
   empty `## [Unreleased]` above; at the bottom, `[Unreleased]` compares
   `vX.Y.Z...HEAD` and `[X.Y.Z]` the previous tag with `vX.Y.Z`.
3. `.\scripts\build.ps1 test`, then commit `Release X.Y.Z`, its body saying
   what the release is and why this bump.
4. `.\scripts\build.ps1 release` from that commit: clean tree, stamped commit,
   `dist\` with the installer, the zip and the recipes zip. Read the whole
   output down to `release OK`.

## 3. Publish

1. Push the branch, open the PR, wait for CI, merge.
2. Tag the merge commit and push the tag alone:
   `git tag vX.Y.Z <merge commit>`, `git push origin vX.Y.Z`.
3. The workflow builds and publishes. Check the release page: installer, zip,
   recipes zip, `SHA256SUMS.txt`, and the changelog section as the notes.

## 4. After

- This machine updates as a user would: *Check for updates*, *Download and
  install*; read the `update` lines and the new version's start in the log.
- The second machine: the maintainer updates it from the menu and brings its
  log back.
- The field checks that waited for this release, listed in memory and in the
  lot pages.
- One line in `.local/journal.md`.
