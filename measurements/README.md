# Measurements

Recorded results from the long-running measurements in `tests/`, kept so a later run can
be read against an earlier one rather than replacing it.

## What is here

`performance-matrix-<conversation>.tsv` — both engines over eleven profiles, produced by
`tools/measure-matrix.sh`. One row per profile:

| column | meaning |
|---|---|
| `conv`, `entries` | the conversation group and its size |
| `profile` | how much of the group the profile has read (see `tests/performance_matrix.rs`) |
| `unseen` | how many entries that leaves unread |
| `fwd_verdict` | `found`, `not-there`, or `gave-up` — the explicit crawl |
| `fwd_ms`, `fwd_states` | what it cost |
| `bwd_verdict` | as above, plus `no-room` when the diagram filled its budget |
| `bwd_ms`, `bwd_nodes` | what it cost |
| any column `CRASHED` | that row took its process down; its log says how |

Both engines get the same allowance — 256 MB and 60 seconds — which is the only way the
two verdicts mean anything against each other. See de-e23q.

## Why the rows are committed and the logs are not

The rows are small, stable and worth diffing: a change that makes the backward search fit
where it did not is visible as a line changing from `no-room` to `found`, and that is
exactly the kind of thing worth noticing in review. The logs are raw cargo output, far
larger, and local.

## Where the logs are

Under `logs/`, one folder per run:

    logs/2026-09-04_1e08319064b7bd9d115f26c3abf35145d3fb7d8e_measure_matrix/
        matrix-1030-all-seen.log
        matrix-1030-deepest-1.log
        ...

The folder is named the way every run log in this repo is named - the date, the commit it
measured, the tool and the run - and carries `-dirty` when the tree had been changed since
that commit. See "Run logs" in DEVELOPING.md.

A WHOLE MATRIX RUN IS ONE MEASUREMENT, so its row logs live or die together: the folder is
what a recorded TSV points back to. They used to be flat files named for the row, which
meant the next run overwrote them and every TSV but the newest had nothing behind it.

`tools/measure-symbolic.sh` writes here the same way, a folder per run named for the
measurement it ran. It used to write under `target/`, where a `cargo clean` took the
measurements with it.

## Regenerating

    tools/measure-matrix.sh              # every conversation
    tools/measure-matrix.sh 368 631      # just these

One row per process, because a row can take the process down with it - see the script.
Expect the better part of an hour for the whole set: the heavy groups spend the full
sixty seconds on several rows.
