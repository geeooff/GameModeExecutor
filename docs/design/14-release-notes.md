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

## Who writes it — the maintainer's questions, 2026-09-18

The page above says *by hand, in the release commit*. The maintainer's
reading, the same day, is that this is a task for the coding agent, not
for a person: a person forgets, and an agent can establish everything a
changelog needs from the commits between two tags and the design pages
they touch, and turn it into something coherent that a lay user can read.
Two constraints shape how, and both are noted here to be settled when the
lot is taken:

- **The release workflow cannot call the agent.** A GitHub runner has
  Copilot, not Claude, and the agent's memory of this project is local to
  the maintainer's machine. So the notes cannot be written *by* the
  release. What the workflow can do is publish a **placeholder** — the
  install lines, the checksums, the commits — and mark the notes as
  pending; the agent then rewrites them on the maintainer's request, from
  the commits and the design record, and the maintainer publishes the
  result. Whether that rewrite goes through `gh release edit` by the agent
  on approval, or through a `CHANGELOG.md` the next release picks up, is
  the choice to make.
- **The rules of the summary belong in `AGENTS.md`.** What a changelog
  line is made of — what the user sees, in sentences, never the code;
  which commits are one line and which are none; when a lot page is
  linked and when nothing is; how the *For the curious* list relates to
  the lines above it — has to be written down before an agent is asked to
  follow it twice the same way. A framework there, the way the log
  contract and the commit-message shape already are.

Left open on purpose until then. What the page decided above stands where
it does not depend on the author: the notes sit first, the commits stay
below, published releases are rewritten once.

## To settle when it is taken

- The two questions above.
- Whether the updater should show the notes itself one day, through the
  API's `body`, rather than open the page. Not before the notes are worth
  showing.
