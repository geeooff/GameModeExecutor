# Lot 11 — Documentation for the people who use it

**Goal.** Someone who has never seen this repository can get their own commands
running in five minutes, and someone curious can find out how it works without
reading Rust.

**Done when:** the two questions a newcomer actually asks — "how do I make it
do my thing" and "why are there two .exe files" — are answered without opening
the source. Closed 2026-09-14.

- [x] `docs/getting-started.md` — features and the shortest path to a result
- [x] `docs/recipes/` — one folder per worked example, each with a ready-made `config.toml`
- [x] `docs/how-it-works.md` — the mechanism, for a curious non-programmer
- [x] FanControl lives in its recipe and nowhere else in the user documentation
- [x] Every shipped `config.toml` passes `validate` — they are files people copy, not illustrations
- [x] Every relative link in `docs/` is checked by the release script

## Numbered last, written early

It sits at the end of the numbering because a third renumbering would have
cost more than it bought. It was written as soon as lots 1 to 5 were done,
because documentation left until last is documentation written from memory,
and memory is where confident, wrong sentences come from. Everything in these
pages was measured or exercised in the sessions recorded in this design
record.

## The audiences, kept apart on purpose

| | Reader | Answers |
| --- | --- | --- |
| `docs/getting-started.md` | wants it working | what do I type, where do I put my commands, why is nothing happening |
| `docs/recipes/<job>/` | has a specific job in mind | the whole thing for my case, copy-paste |
| `docs/how-it-works.md` | curious, not a programmer, knows what a process is | how does it know a game is running, why two executables, why the wait after quitting |
| `docs/reference.md` | needs the exact behaviour | commands, exit codes, every configuration field, the log contract |
| `docs/design/` | evaluating or changing the code | why it is built this way, what was measured, what is still open |

FanControl is confined to its recipe on purpose. It is the example that drove
the project, and left loose it would spread through every page until the
program looked like a FanControl accessory rather than something that runs
commands. `getting-started.md` names the *problem* — programs needing
administrator rights — and points at the recipe for the cure.

`how-it-works.md` earns its place because the design has three things a user
will *notice* and misread as bugs — the unpredictable wait after quitting, the
two executables, and a game that is detected but not named. Each has a real
reason; left unexplained, each looks like a defect.

## One folder per recipe

Each recipe is a folder holding its own `README.md` and a complete
`config.toml`, plus whatever else it needs — the FanControl one carries its
two Task Scheduler templates and the script that fills them in. Adding a
recipe adds a folder and one row in the index; nothing else moves.

The FanControl recipe names its tasks after the two **roles** the watcher
knows, `FanControl Idle` and `FanControl Game`, so the shipped `config.toml`
is the same for everyone; which of the user's FanControl *configurations* —
FanControl's own word for a saved set of fan curves — each task applies is an
argument of the task, asked for by the script from what is saved.

## The maintenance rule

This lot is finished; the documentation is not, and never will be. **Any lot
that changes what the user sees updates these pages as part of being done** —
not as a follow-up, in the same commit. The alternative is documentation that
describes a program that no longer exists, which is worse than none, because
nobody distrusts it until it has already wasted their afternoon.
