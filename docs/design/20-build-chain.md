# Lot 20 — The build chain, official and kept current

**Status: proposed and decided 2026-10-17, to take next**, while Lot 19's
spike waits for an Office update. Two parts, in this order: Dependabot keeps
current whatever the first part settles on.

## 1. Official actions, current practice

Read on 2026-10-17: CI uses `actions/checkout@v7`, `actions/upload-artifact@v7`
(GitHub's), `dtolnay/rust-toolchain@stable` and `Swatinem/rust-cache@v2`
(community); the release publishes with `gh`.

- Rust from `rustup`, already on GitHub's Windows runners, with the toolchain
  named in `rust-toolchain.toml`, which pins it on the developer's machine too,
  instead of `dtolnay/rust-toolchain`.
- The cache: `Swatinem/rust-cache` or `actions/cache` with an explicit key,
  decided on CI time measured with each.
- Actions named by version tag, not by commit — the maintainer's choice on
  2026-10-17: a tag is a release its publisher calls stable, and Dependabot
  moves it.
- Least privilege: `contents: read` for CI; the release keeps
  `contents: write` and adds what the attestation needs.
- `timeout-minutes`, and a `concurrency` group that cancels a superseded run.
- The release's files attested with `actions/attest-build-provenance`: GitHub
  signs, for each file, that this workflow built it from this commit, and
  `gh attestation verify <file> --repo geeooff/GameModeExecutor` checks it.
  Not a code signature — SmartScreen still warns — but proof the installer
  was built from the public source, which `SHA256SUMS.txt`, published on the
  same page as the files, cannot give.

## 2. Dependabot and automatic merges

- `.github/dependabot.yml` for `cargo` and `github-actions`, weekly, compatible
  updates grouped in one PR per ecosystem.
- Merged automatically only when Cargo calls the update compatible: a patch,
  or a minor for a crate at 1.0 or later. A 0.x minor — the `windows` crate's
  0.62 to 0.63 — or a major stays a PR for the maintainer.
  `dependabot/fetch-metadata` reads the update type; a workflow asks
  `gh pr merge --auto`.
- `main` protected by a rule requiring the CI check, without which an
  automatic merge does not wait for it: `allow_auto_merge` is on and `main`
  unprotected, read on 2026-10-17. A change to the repository's settings, so
  with approval.
- An audit of `Cargo.lock` against RustSec's advisories in CI, so a vulnerable
  crate fails the build without waiting for an update.
- `AGENTS.md`: an automatic merge is a standing approval for that class of
  update and nothing else.

**Done when** CI and the release use official actions or justified ones, by
tag, with least privilege; a release's files carry a verifiable attestation;
Dependabot opens grouped PRs for crates and actions, the compatible ones
merging themselves after a green CI; the audit runs in CI.
