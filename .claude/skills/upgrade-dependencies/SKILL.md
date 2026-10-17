---
name: upgrade-dependencies
description: Upgrade GameModeExecutor's dependencies across breaking versions — crate minors in 0.x and majors, new major versions of GitHub Actions, what a new stable toolchain asks — following each upgrade guide, without workarounds. Use when the maintainer asks to update the dependencies, or for a Dependabot PR left waiting.
---

# Upgrading the dependencies

Dependabot merges by itself what Cargo calls compatible: patches, and minors
of crates at 1.0 or later. Everything else comes here — a 0.x minor such as
`windows` 0.62 to 0.63, a major, an action's new major, the lints of a new
stable toolchain. Every push and merge needs the maintainer's approval.

## 1. What is behind

- **Crates**: for each direct dependency in `Cargo.toml`, `cargo info <crate>`
  run in the repository prints `version: <locked> (latest <newest>)` when a
  newer one exists. (`cargo update --breaking` is unstable on stable Cargo.)
- **Actions**: each `uses:` in `.github/workflows/` against its publisher's
  latest release, `gh api repos/<owner>/<repo>/releases/latest --jq .tag_name`.
  Actions are named by tag, the maintainer's choice.
- **Toolchain**: `rustup update`, then `.\scripts\build.ps1 test`; a new
  clippy lint failing under `-D warnings` is an upgrade like any other.
- **Dependabot's open PRs**, `gh pr list --author app/dependabot`, list what
  waits. The work is done here on a branch; Dependabot closes its PR once
  `main` has the version.

Show the maintainer the list — locked, latest, kind of change — and agree on
what to take.

## 2. One dependency at a time

On `deps-YYYY-MM-DD`, from `main`, one commit per dependency, or per family
that moves together (`windows` and `windows-registry`):

1. **Read the upgrade path first**: the changelog, release notes or migration
   guide of every version between the locked one and the target, from the
   publisher — the crate's repository, docs.rs, the action's releases. Note
   each breaking change this code meets.
2. Raise the requirement in `Cargo.toml`, or the tag in the workflow, then
   `cargo update -p <crate>`.
3. Fix each break **the way the guide says**: the new API, the replacement it
   names. Never: a lint silenced (`#[allow(deprecated)]` and the like), a
   `[patch]` or a fork, a version held below the target, the old code copied
   in, a feature turned off to dodge a change. Keep the code like its
   neighbours, and module docs and `// SAFETY:` comments true: `windows`
   upgrades move signatures under `unsafe`.
4. `.\scripts\build.ps1 test`; commit only on `test OK`.
5. When the upgrade can change what runs — `windows`, `tracing*`, `time`, the
   toolchain — build a release, install it as `AGENTS.md`'s *Verify as the
   user runs it* says, read the log through a real game, and measure the
   watcher at rest against the last release (`presence-probe footprint`,
   `cost`).
6. An upgrade that cannot be done properly — an upstream bug, a missing
   replacement — is not forced: stop, write down why, ask the maintainer.

The commit gives the versions, what broke, how it was fixed, and the guide's
link. The changelog gets a line only for what a user would notice.

## 3. Hand over

Push the branch, open one PR listing each upgrade with its guide, wait for
CI, merge — the push and the merge approved. An action used only by
`release.yml` is proven at the next release; the PR says so.
