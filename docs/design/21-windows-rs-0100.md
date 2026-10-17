# Lot 21 — Moving to windows-rs 0.100

**Status: proposed 2026-10-17, waiting for `windows` 0.100 on crates.io.**
windows-rs moved its smaller crates to 0.100.0 on 2026-09-03 (its release
74) and held back `windows` and `windows-sys` while it tests their new
metadata ([microsoft/windows-rs#4867](https://github.com/microsoft/windows-rs/issues/4867)).
The program uses `windows` 0.62 throughout and `windows-registry` 0.6; they
move together, once, following the `upgrade-dependencies` skill.

## Why a lot of its own

The 0.100 bindings come from the Windows SDK headers instead of the
Win32Metadata project, and the publisher lists what that breaks:

- features and modules renamed after SDK headers (`Win32_*` become lowercase
  header names): every feature in `Cargo.toml` and every import;
- signatures as the headers declare them: raw pointer and count parameters,
  `BOOL` and `HRESULT` returned as such;
- generated enum and ownership types removed where the SDK has primitives —
  `HLOCAL`, for one, no longer frees itself;
- record layouts as the headers lay them out.

Nearly every `unsafe` call and its `// SAFETY:` comment is touched, so this is
the riskiest dependency change the program has had.

## Until then

`windows-registry` stays at 0.6. Tried on 2026-10-17: 0.100 builds with no
change to the code and adds about 1.5 KB, but brings a second generation of
`windows-result`, `windows-strings` and `windows-link` beside the ones
`windows` 0.62 uses, and needs Rust 1.95 where `rust-version` says 1.88 —
for nothing the program uses. Decided with the maintainer the same day: wait.

## When `windows` 0.100 lands

- Read the publisher's migration notes and the crates' documentation first.
- One commit per area of the program, each through `build.ps1 test`; no
  workaround, as the skill says. `rust-version` follows the family's minimum.
- In the field, every path that calls Windows: detection and the presence
  writer's wait, the games marked by hand, the GPU counters, the icon, menu
  and notifications, the session end, the update and `purge`.
- The watcher measured at rest and in a game against 0.4.0.

**Done when** the program builds on `windows` and `windows-registry` 0.100
with no crate of the 0.62 generation left, every check passes, a field
session has exercised each path above, and the footprint is recorded.
