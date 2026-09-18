# Lot 14 — Release notes people can read

**Status: done 2026-09-18, the day it was proposed**, on the maintainer's
remark after the first update ran through the menu: *What changed in
0.1.0* opened the release page, and the page said *First public release*
over a list of commits. That is a changelog for the people who wrote the
commits, not for the person who clicked. Taken before 0.2.0, so that
release read well from the moment it was published — it did, and 0.1.0's
page was rewritten the same evening.

- [x] `CHANGELOG.md`, with the sections for 0.1.0 and what 0.2.0 will carry — 2026-09-18
- [x] `release-notes.ps1` takes the section for the tag and refuses a version without one; `build.ps1 release` refuses it first, on the machine that can still write it — 2026-09-18
- [x] The rules of a section, in `AGENTS.md` — 2026-09-18
- [x] `v0.1.0`'s notes rewritten once with its section, on approval — 2026-09-18, the platform line corrected with it
- [x] The first release published this way: 0.2.0 — 2026-09-18, the workflow's first run with a dated section, green

**Goal.** Every release page reads, in a few lines, what changed for the
person running the program — and the same lines are what the updater's
*What changed* opens.

**Done when:** a release published by the workflow carries notes a lay
user can read without a link, the commits stay below for the curious, and
the notes of every release already published have been rewritten the same
way.

## What is decided, ahead of building it

- **The notes are written before the release, not by it.** The workflow
  cannot summarise. The first draft of this page said *by hand, in the
  release commit*; the maintainer's questions below move the author to
  the coding agent and the moment to *on request*. The shape holds either
  way: one section per version, newest first, in the form
  [Keep a Changelog](https://keepachangelog.com/) made familiar — *Added*,
  *Changed*, *Fixed*, *Removed* — each line a sentence about what the user
  sees, not about the code.
- **A release is never published with nothing to say — or it says so.**
  Either the workflow refuses a tag whose version has no section, the way
  it refuses a tag that disagrees with `Cargo.toml`, or it publishes a
  placeholder that says the notes are pending. Which one follows from the
  questions below.
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

## Who writes it — the maintainer's questions, and the answer

The first draft of this page said *by hand, in the release commit*. The
maintainer's reading, the same day, was that this is a task for the coding
agent, not for a person: a person forgets, and an agent can establish
everything a changelog needs from the commits between two tags and the
design pages they touch, and turn it into something coherent that a lay
user can read. Two constraints were raised with it:

- **The release workflow cannot call the agent.** A GitHub runner has
  Copilot, not Claude, and the agent's memory of this project is local to
  the maintainer's machine. So the notes cannot be written *by* the
  release; a placeholder the agent fills afterwards was one shape
  considered.
- **The rules of the summary belong in `AGENTS.md`**, written down before
  an agent is asked to follow them twice the same way.

**Settled the same evening, by noticing where the release commit is made:**
on the maintainer's machine, with the agent present. Nothing needs the
runner to summarise. The agent writes the section in `CHANGELOG.md` — in
`[Unreleased]` as the work lands, at the latest in the release commit —
from the commits since the previous tag and the design pages they touch;
the maintainer reads it as a diff in the release pull request, the way any
change is read; the workflow copies it onto the release page and refuses a
version without one, so no release is ever published with nothing to say.
No placeholder, no edit after publication. The rules are in `AGENTS.md`
beside the release procedure: what a line is, what earns none, when to
link, and that a published section is history.

The one exception is the release published before this lot: `v0.1.0`'s
notes are rewritten once, by `gh release edit` on the maintainer's
approval, with the section the changelog now carries for it.

## To settle later

- Whether the updater should show the notes itself one day, through the
  API's `body`, rather than open the page. Not before the notes are worth
  showing.
