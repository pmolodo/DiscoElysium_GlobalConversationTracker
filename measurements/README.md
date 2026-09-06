# Measurements

The long-running measurements themselves, and under `logs/` what they produced. Nothing
under here is committed except the measurements and this file.

## Why they are not in `tests/`

A test passes or fails on its own; a measurement produces a number a person reads. The two
were being confused - this repository has already had a measurement's figures quoted as
though a test had verified them - and a measurement that asserts nothing still looks green
when it is run, which is what invites that mistake. See de-18bo.

They are Cargo EXAMPLES, named in `Cargo.toml` with an explicit `path` because Cargo looks
for examples in `examples/` and these live beside their logs. Being examples also takes
them out of `cargo test`, which used to compile and link every one of them on the way to
running none of them.

**Not everything here produces a number**, and the directory's name undersells it. Two of
its contents are a different kind of thing, and they are here because the split that
matters is "runs in the suite" against "run by hand" - a second directory for two files
would be a distinction nobody remembers:

- `static_analysis.rs` is a GENERATOR. Two of its three passes write the committed files
  under `analysis/`. Nothing in the repository reads those yet - the prefilter they exist
  for was measured and reverted, see de-asw.3 - so a stale one is not by itself a bug.
- `guard_snapshot.rs` is a SNAPSHOT. It dumps every parsed guard to the file named by
  `GUARD_SNAPSHOT`, to be run on both sides of a parser change and diffed. Identical output
  over 26,210 guards is the equivalence claim; a diff is the list that changed meaning.

What they have in common with the measurements is the thing that got them moved: none of
them passes or fails on its own, so none of them belongs in `tests/`.

Run one the way anything slow is run here - through `tools/run-logged.sh`, so there is a
log to read afterwards:

    tools/run-logged.sh cargo answers -- cargo run --release --example symbolic_answers

    RUN_LOG_DIR=measurements/logs CONVERSATION=14 \
      tools/run-logged.sh cargo slots -- cargo run --release --example layout_shape -- slots

`layout_shape` is the one that takes an argument: `groups` (the default) counts the slot
classes per group, `slots` lists one group's slots so the classifier behind the counts can
be read rather than trusted.

## Where a run's results are

Under `logs/`, one folder per run, holding both the raw output and the summary drawn from
it:

    logs/2026-09-04_1e08319064b7bd9d115f26c3abf35145d3fb7d8e_measure_matrix/
        matrix-1030-all-seen.log        <- one log per row
        matrix-1030-deepest-1.log
        ...
        performance-matrix-1030.tsv     <- the rows for that conversation
        performance-matrix-14.tsv
        ...

The folder is named the way every run log in this repo is named - the date, the commit it
measured, the tool and the run - and carries `-dirty` when the tree had been changed since
that commit. See "Run logs" in DEVELOPING.md.

A WHOLE MATRIX RUN IS ONE MEASUREMENT, so its logs and its rows live or die together. The
logs used to be flat files named for the row, which meant the next run overwrote them and
every recorded TSV had nothing behind it except the newest one's.

`tools/measure-symbolic.sh` writes here the same way, a folder per run named for the
measurement it ran. It used to write under `target/`, where a `cargo clean` took the
measurements with it.

It runs a symbolic measurement ONE CONVERSATION PER PROCESS, which is what keeps a crash
costing one row rather than every row after it:

    tools/measure-symbolic.sh shared_symbolic 631
    tools/measure-symbolic.sh backward_support 368 631
    tools/measure-symbolic.sh money_and_clock shipped 28

The first argument is the example's name. A measurement with several stages behind one
`main` takes the stage second; anything that looks like a number is read as a conversation,
so the stage can be left out.

## Why nothing here is committed

The TSVs used to be, on the argument that the rows are small and worth diffing. They are
not worth diffing: a row is a wall-clock time on one machine, and it moves whenever
anything about the search moves - the memory allowance, the width of a state, which
machine ran it, what else that machine was doing. A committed baseline like that is wrong
far more often than it is right, and wrong SILENTLY, because nothing re-runs it to find
out. A reader who trusts it is worse off than one who has nothing.

What replaces it is comparing two runs: two folders, each holding its own rows next to the
logs those rows came from, each stamped with the commit it measured. That is the honest
shape of the comparison, and it makes the question "against what?" impossible to skip.

## What a row says

| column | meaning |
|---|---|
| `conv`, `entries` | the conversation group and its size |
| `profile` | how much of the group the profile has read (see `measurements/performance_matrix.rs`) |
| `unseen` | how many entries that leaves unread |
| `fwd_verdict` | `found`, `not-there`, `gave-up`, plus `no-room` when the diagram filled its budget and `no-ram` when the machine did |
| `fwd_ms`, `fwd_nodes`, `fwd_setsum` | what it cost: manager nodes held, and the per-entry sets summed |
| `bwd_verdict` | as `fwd`, except that there is no `no-ram`: its manager is allocated up front, so a machine that cannot supply the budget gives `NOT-MEASURED` before anything runs |
| `bwd_ms`, `bwd_nodes` | what it cost |
| `bwd_asked`, `bwd_cands` | candidates asked about, out of candidates waiting - one fixed point was paid per candidate asked |
| `fwdbwd_verdict` | the same, for the switching method the game actually runs |
| `fwdbwd_ms`, `fwdbwd_by`, `fwdbwd_asked` | what it cost, and which half answered - `Forwards` where the slice halted, `Backwards` where the driver settled, `Partly` where it did not and the answer is a lower bound |
| any column `CRASHED` | that row took its process down; its log says how |
| any column `NOT-MEASURED` | the row never ran; see below |

### The columns are two searches and the method that switches between them

`fwd` walks links from the start, a decision diagram per entry. `bwd` computes pre-images
from a target, one candidate at a time, stopping at the first candidate proved reachable.
`fwdbwd` is what the game actually runs: a forward slice hunting the best class anything
reachable carries, and where that does not answer, the backward driver told what the slice
found. The first two are its halves measured alone, which is what makes the third readable.

**READING AN OLDER RUN, and the names have moved twice.**

| the columns say | what they were |
|---|---|
| `fwd`, `bwd`, `fwdbwd` | today's, all symbolic |
| `explicit`, `symfwd`, `symbwd` | older names. `symfwd` is today's `fwd` and `symbwd` today's `bwd`; `explicit` measured a state-at-a-time search, which nothing measures now |
| `fwd`, `bwd` and nothing else | older still. `fwd` is the state-at-a-time search and `bwd` is today's `fwd`; no backward search was measured at all |

So a `fwd` column means opposite things at the two ends of that table, and the way to tell
is what else is in the header. `tools/matrix-remaining.awk` decides it the same way when it
reuses old rows as weights.

Not every run holds all three. `ENGINES` narrows the selection and the header follows it,
so a narrowed run is a narrower row rather than a wide one with holes; read the header
rather than assuming the columns.

Every engine in a run gets the same allowance, which is the only way the verdicts mean
anything against each other: the shared measurement budget in `DiagramBudget::measurement()`,
plus a time cap that is meant not to be what stops a row. See de-e23q and de-z5sp.

### `no-room`, `CRASHED` and `NOT-MEASURED` are three different things

They look alike from outside - the row has no numbers in it - and they mean opposite
things, so the run keeps them apart.

- `no-room` is a RESULT, and at six gigabytes an interesting one: the search was given
  every byte it was allowed and still had no answer.
- `CRASHED` is a result too. The row took its process down; the log says how, and that is
  a fact about the search.
- `NOT-MEASURED` is not a result at all. The machine could not supply the budget, so
  nothing ran and there is nothing to learn - the row wants running again when the memory
  is free. A run holding any of these is not yet a measurement, and the script says so at
  the end rather than leaving it to be noticed.

The distinction needs asking BEFORE the memory is spent, because spending it has no
failure path: the diagram manager preallocates its node store with `Vec::with_capacity`,
which aborts the process rather than returning an error. `DiagramBudget::can_be_supplied`
reserves the same bytes fallibly first.

## Regenerating

    tools/measure-matrix.sh              # every conversation
    tools/measure-matrix.sh 368 631      # just these

One row per process, because a row can take the process down with it - see the script.
Expect the better part of an hour for the whole set: the heavy groups spend the full time
cap on several rows.
