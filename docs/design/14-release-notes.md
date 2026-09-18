# Lot 14 — Release notes people can read

**Status: proposed 2026-09-18**, on the maintainer's remark after the first
update ran through the menu: *What changed in 0.1.0* opened the release
page, and the page said *First public release* over a list of commits.
That is a changelog for the people who wrote the commits, not for the
person who clicked.

**Goal.** Every release page reads, in a few lines, what changed for the
person running the program — and the same lines are what the updater's
*What changed* opens.

**Done when:** a release published by the workflow carries notes a lay
user can read without a link, the commits stay below for the curious, and
the notes of every release already published have been rewritten the same
way.

## What is decided, ahead of building it

- **The notes are written by hand, in the release commit.** The workflow
  cannot summarise; a person can, once per release, in the commit that
  bumps the version — which is already the one commit a release comes
  from. A `CHANGELOG.md` at the root, one section per version, newest
  first, in the shape [Keep a Changelog](https://keepachangelog.com/) made
  familiar: *Added*, *Changed*, *Fixed*, *Removed*, each line a sentence
  about what the user sees, not about the code.
- **The workflow refuses a release without its section.** The tag names
  the version; `release-notes.ps1` takes the section for that version out
  of `CHANGELOG.md` and fails the run when there is none — the same way it
  already fails when the tag and `Cargo.toml` disagree. A release cannot
  be published with nothing to say.
- **The commits stay, below.** The list the script already writes goes
  under a *For the curious* heading, after the notes, unchanged: it is
  the honest record and costs nothing.
- **Links, sparingly.** A line may link the lot page in `docs/design/`
  that carries the reasoning, when there is one; commits are not linked
  from the notes, they are listed below. No link is needed for a line to
  make sense.
- **The install lines and the checksums stay where they are**, first and
  last, as the release notes script writes them today.
- **Published releases are rewritten once**, by hand, with the new shape,
  when this lot lands — `v0.1.0` and whatever follows it before then.

## To settle when it is taken

- Whether the updater should show the notes itself one day, through the
  API's `body`, rather than open the page. Not before the notes are worth
  showing.
- Where the standing rule goes: *a release commit updates `CHANGELOG.md`*
  belongs in `AGENTS.md` beside the versioning rule.
