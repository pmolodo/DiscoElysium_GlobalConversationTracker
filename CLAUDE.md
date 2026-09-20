# Working in this repository

## Running commands: tee anything that might be slow, and say where the log is

**Anything that might even potentially be slow goes through `tools/run-logged.sh`, and the
reply says the log path.** Builds, test runs, measurements, the extractor, deploys, any
script whose runtime is not obviously instant. When in doubt, log it: the cost is one
wrapper, and the run that turns out to be worth reading is rarely the one that looked like
it would be.

Genuinely instant things - `ls`, `git status`, reading a file - do not need it.

```sh
tools/run-logged.sh --kind testing cargo full-suite -- cargo test --release
tools/run-logged.sh --kind performance cargo menu-761 -- \
  cargo run --release --example menu_matrix -- --conversation 761
```

**`--kind` says which tree the log belongs in**, and the three are named the same way:

```text
performance  performance/logs  timing: numbers to compare against other numbers
testing      testing/logs      correctness: suites, in-game runs and builds
analysis     analysis/logs     data: what some other run decided, read back
```

**EVERY RUN IS A MEASUREMENT** - of correctness, of data, or of timing - which is why none of
the three is called `measure`: the kind says WHAT IS MEASURED. Each one is also its tree's
name, so a log's kind and its path say the same word.

THE TOOL NAME CANNOT DECIDE IT, which is why it is said rather than inferred: `cargo
full-suite` measures correctness and `cargo walk-1467` measures timing, and both are cargo.
Only the caller knows which.

**THE KIND DECIDES MORE THAN THE TREE: only `performance` pays the cold-run tax.** A throw-away
first pass protects a comparison between TIMINGS, and a test or an analysis pass produces none -
so they skip it and cost half as much. The kind reaches the driver as `DEGCT_RUN_KIND`, and a
perf tool pressed into deriving a dataset says so itself:

```sh
tools/measure-menus.py --kind analysis --runs 1 all   # which groups fall through; no cold pass
```

`--kind` on the tool beats the variable, and with neither the run counts as timing and
pays - skipping a discard that was wanted corrupts a comparison silently, while taking one that
was not wanted costs a run.

It tees the whole output to `<tree>/<date>/<date>_<time>_<revision>_<tool>_<verb>.txt`
(`RUN_LOG_DIR` moves that) and passes the command's own exit status straight through, so it
drops in wherever the bare command stood. `--name-only` prints the path without running
anything.

A measurement's ROWS take the transcript's name exactly, minus the extension, so which folder
belongs to which log is readable without opening either. A label - `--out qy5t-before`
- is a SUFFIX on that name rather than a name of its own, `..._menus__qy5t-before/`, so a run
can say what it was for without giving up the pairing. `tools/tidy-logs.py` sorts any log that
arrives loose, by the date in its name or its modification time.

The rows go under the tree the KIND names, the same tree the transcript goes to, so a dataset a
performance tool derived is filed with the datasets and not among the timings.

Analysis tools write their DATA to `analysis/outputs/`, beside their transcripts in
`analysis/logs/`.

**Never pipe such a command into `grep`, `tail`, `head`, `sort` or anything else as the
only thing that receives its output.** A filter keeps the few lines it matched and throws
the rest away, so a run that took forty minutes leaves nothing to look at when the
interesting part turns out to be somewhere the filter did not look - a panic, a progress
line, the row before the one that failed. Filter the LOG afterwards instead; it is still
there.

Say the log path in the reply, every time, so a run can be followed while it is still
going rather than waited out. A run whose log path was never mentioned is one nobody else
can watch.

**Give the path WHOLE and copy-pastable.** These names carry a full commit hash, so the
temptation is to elide the middle - `2026-09-05_03,51,18_..._cargo_full-suite.txt`. Do not:
an elided path cannot be pasted into anything, which defeats the point of saying it. Print
it verbatim, on its own line, in a code block rather than inline prose.

This file and `AGENTS.md` carry the same rules and are kept in step. They are duplicated
rather than one pointing at the other because the Git rule below overrides a built-in
default, and an override that goes missing because a pointer was not followed is worse
than two copies to keep aligned.

## Environment variables: name every one of ours `DEGCT_`

**Every environment variable this project defines is prefixed `DEGCT_`.** All languages, all
scripts - Rust, Python, bash, PowerShell.

**A throwaway script in a session scratchpad uses `DEGCTT_` instead** - the same rule with a
second `T` for temporary. Temp scripts are where the convention is most tempting to skip and
where the bug is hardest to see: nobody reviews them, nothing tests them, and they are deleted
before anyone asks what went wrong. The separate prefix also keeps them visibly apart from the
committed drivers, so a stray value exported by a scratch script can never be mistaken for a
setting one of the real ones reads.

Variables we merely READ from the world keep their own names: `PATH`, `CARGO_TARGET_DIR`,
`NUMBER_OF_PROCESSORS`. The rule is about names we invent.

**Why, and it is not hypothetical - it has cost time three times.** `GROUPS` is a bash
built-in array holding the user's numeric group ids. Assigning to it looks like it works, and
`set -x` will even show the assignment with the right value; every later `"$GROUPS"` then
expands to `${GROUPS[0]}`, a gid. In this repository that is a number the measurement takes
for a conversation id, so a run refuses with "conversation 197609: no group builds from it"
and nothing points at the variable.

The shell owns a long list of short generic names - `GROUPS`, `IFS`, `HOME`, `LINES`,
`COLUMNS`, `SECONDS`, `RANDOM`, `PWD`, `REPLY`, `PIPESTATUS`, `UID`, `HOSTNAME` - and they are
drawn from exactly the vocabulary a measurement wants. A prefix takes the whole class of
collision off the table rather than dodging them one at a time.

See de-12wr.9 for the rename of the names that predate this rule.

## One algorithm by default, everywhere it runs

**Performance measurements, in-game runs, and the in-game and offline test engines all use the
algorithm the product ships, by default.** Whatever the plugin sends and the engine does with it -
the walk a request carries and the hub cut it drives included - a default measurement row, a
default in-game suite and a default offline test do the same.

A different algorithm is for a comparison someone asked for, and is opt-in: a named arm such as
`--marking hybrid-spent`, never the default. When the shipped algorithm changes, every default path
changes with it in the same piece of work - otherwise a green test or a measured number describes
code the game does not run. The case that made this a rule: the menu matrix kept measuring menus
with no walk after the shipped marking started cutting by one (de-r2xf.11).

## Two algorithms marking a menu differently is not a defect in either

**What a marking computes is GUIDANCE - which options lead the player towards content they have
not seen - and that is a loosely defined goal rather than a function with one right answer.**
Soonest by which measure of soonest? Towards which unseen thing, when they are not all worth the
same? Spending how many of the menu's options to get there? Two algorithms can weigh those
differently, both be a reasonable answer to where the player should go next, and star different
options. Neither is wrong for disagreeing with the other.

So a `starred` column that differs between two arms says the searches took different routes, or
answered slightly different questions - not that one of them has a bug. What produces such a
difference:

- **Tie-break order, which is mostly just encounter order.** A round takes the first result it can
  be sure nothing is LESS than, so among equals whichever arrives first wins, and a different
  search arrives at a different one first. `menu::tied_targets_choose_one_winner_per_round` is
  that case in miniature.
- **A different distance metric.** Two arms that measure "soonest" differently are answering
  slightly different questions, and each can be answering its own correctly.
- Anything else that moves the order results are met in, or the bound they are accepted against.

AND THE COUNT IS NO SHORTCUT EITHER. A different NUMBER of options starred is no more a defect
than a different set: how many a menu marks falls out of how many rounds settled and what each
round claimed, and an arm that reaches two targets through one option marks fewer than one that
spends an option on each - which is a different idea of good guidance, not a worse execution of
the same one. No column of this row lets a reader compare two arms and conclude a defect from the
comparison alone.

WHAT A DIFFERENCE IS GOOD FOR is asking which question each arm answered, and which of the two we
would rather the player's menu answered. That is a judgement about guidance, taken by reading both
answers against the dialogue. `tests/menu_oracle.rs` compares a marking against exhaustive
concrete-state distances, so it settles whether an arm computes the distances IT claims - it does
not make one arm's idea of guidance the standard the other has to meet.

Two runs of the SAME arm differing is worth a look, but it is not proof of a defect either: a menu
answered under a wall gives up where the wall falls, so a run that was interrupted at a different
moment settles differently. `settled` and `partly` say whether that is what happened, and a
difference among rows that all settled everything is the one that has no such explanation.

None of this weakens the reason the column exists - de-2p8j.2, where a change that keeps the count
while moving which options a menu recommends would read as no change at all in `rounds`. That is
the shipped algorithm against ITSELF across a change, where no second search order is available to
explain a difference away. The case that made this a rule: 761 stars 422,164,989,848 under the
shipped arm and 422,416,164,989 under `--pooled-rounds`, and the pair was written up as a defect
in one of them before anything asked what else could produce it (de-mqu3).

## Committed saves: never edit what the game wrote

**A save that came from the game is never edited - above all one made in a real playthrough.**
Not to make it agree with itself, not to match what the game would hold once it has loaded
it, not to make a test pass. It is the record of a world the game was actually in, and an
edited copy tests a world no run of the game was ever in. A scenario save built on purpose
as a diff of another says what it changes in its diff; that is how it is made, not a licence
to alter a save the game wrote.

**Where the game always changes something on the way in, simulate that step in the reader**
(`tests/common/fixtures.rs`), so the committed save stays the game's and the world a test
builds is the loaded one. The case met so far: `WeatherController` rewrites
`auto.is_raining` and `auto.is_snowing` from the weather preset on load, and a real save can
be written with the two disagreeing.

**Everything a fixture needs is in the save, because a save is the whole game state.** Where
a fixture world cannot answer something, find where the save records it and read it - Kim's
party membership is `partyState.isKimInParty` in the first blob - rather than assuming a value
or stating one beside the scenario row.

## Git

### Top-level agent

**Commit to the branch that is checked out**, including when that branch is `main`. Do
not create a branch first.

This is written down because Claude Code's built-in default is the opposite - "if on the
default branch, branch first" - and it does not suit this repository, whose history is a
single line of commits on `main`. A branch per change is noise here rather than
isolation.

### Sub-agents

Always work on your own branch in your own worktree.
