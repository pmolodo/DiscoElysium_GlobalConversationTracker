# Measurements

Where the long-running measurements in `tests/` put their results. Nothing under here is
committed except this file.

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
| `profile` | how much of the group the profile has read (see `tests/performance_matrix.rs`) |
| `unseen` | how many entries that leaves unread |
| `fwd_verdict` | `found`, `not-there`, or `gave-up` - the explicit crawl |
| `fwd_ms`, `fwd_states` | what it cost |
| `bwd_verdict` | as above, plus `no-room` when the diagram filled its budget |
| `bwd_ms`, `bwd_nodes` | what it cost |
| any column `CRASHED` | that row took its process down; its log says how |

Both engines get the same allowance, which is the only way the two verdicts mean anything
against each other: the shared measurement budget in `DiagramBudget::measurement()`, plus
a time cap the test names. See de-e23q and de-z5sp.

## Regenerating

    tools/measure-matrix.sh              # every conversation
    tools/measure-matrix.sh 368 631      # just these

One row per process, because a row can take the process down with it - see the script.
Expect the better part of an hour for the whole set: the heavy groups spend the full time
cap on several rows.
