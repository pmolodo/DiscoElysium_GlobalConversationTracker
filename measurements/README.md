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

- `static_analysis.rs` is a GENERATOR. Two of its three passes write a group partition and
  a link-reachability table under `analysis/`, from the link structure alone. NOTHING READS
  THEM AND THEY ARE NOT COMMITTED: the prefilter they were built for was measured and
  reverted (de-asw.3), and a checked-in table derived from the index is a second copy of the
  index's shape - wrong, and silently wrong, the first time a group grows an entry. It is
  kept because it is where a structural precomputation would live if one ever pays; run it
  when something wants the answer, and delete what it wrote afterwards.
- `guard_snapshot.rs` is a SNAPSHOT. It dumps every parsed guard to the file named by
  `GUARD_SNAPSHOT`, to be run on both sides of a parser change and diffed. Identical output
  over 26,210 guards is the equivalence claim; a diff is the list that changed meaning.
  NOTHING RUNS IT ON A SCHEDULE - it is reached for when the parser is about to move, and
  it is the person making that change who has to remember it.

What they have in common with the measurements is the thing that got them moved: none of
them passes or fails on its own, so none of them belongs in `tests/`.

Run one the way anything slow is run here - through `tools/run-logged.sh`, so there is a
log to read afterwards:

    tools/run-logged.sh cargo answers -- cargo run --release --example symbolic_answers

    DEGCT_RUN_LOG_DIR=measurements/logs DEGCT_CONVERSATION=14 \
      tools/run-logged.sh cargo slots -- cargo run --release --example layout_shape -- slots

Several take an argument, and it selects a stage rather than a setting. `layout_shape`
takes `groups` (the default), which counts the slot classes per group, or `slots`, which
lists one group's slots so the classifier behind the counts can be read rather than trusted.
`dominance_share` takes `rows` (the default), `menu`, `all` for the whole game, or `verify`,
which re-derives its dominator relation by deleting entries and comparing. `candidate_recurrence`
takes `links` (the default), where a menu's options are one node's own links, `deepest`,
where they come from the adversarial profile - the two disagree on purpose - or `walk`,
successive menus along one group rather than the options of one node. `cacheable_asks` runs
the very asks that last arm counts, over the same walk, so the two numbers multiply.
`dead_quantify` takes `sets` (the default), the forward fixed point over a whole group, or
`menu`, the shipped call over an adversarial menu - and those two can move opposite ways,
which is the reason both are there.

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
    tools/measure-symbolic.sh layout_shape slots 14

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
| `*_setup` | how much of that engine's `ms` went on building the layout, the manager, the compiled guards and the seed rather than on searching. `ms` still carries the whole of it, so the search is the difference |
| `bwd_verdict` | as `fwd`, except that there is no `no-ram`: its manager is allocated up front, so a machine that cannot supply the budget gives `NOT-MEASURED` before anything runs |
| `bwd_ms`, `bwd_nodes` | what it cost |
| `bwd_asked`, `bwd_cands` | candidates asked about, out of candidates waiting - one fixed point was paid per candidate asked |
| `ingame_verdict` | the same, for the switching method the game runs, at the settings a player actually has |
| `nolimit_verdict` | the same method with its limits off, walled at two minutes |
| `ingame_ms`, `ingame_by`, `ingame_asked` | what it cost, and which half answered - `Forwards` where the slice halted, `Backwards` where the driver settled, `Partly` where it did not and the answer is a lower bound, `Gated` where the game would not have searched at all |
| `nolimit_*` | the same four, for the unlimited column |
| any column `CRASHED` | that row took its process down; its log says how |
| any column `NOT-MEASURED` | the row never ran; see below |

### The columns are two searches and the method that switches between them

`fwd` walks links from the start, a decision diagram per entry. `bwd` computes pre-images
from a target, one candidate at a time, stopping at the first candidate proved reachable.
`ingame` is what the game actually runs: a forward slice hunting the best class anything
reachable carries, and where that does not answer, the backward driver told what the slice
found. The first two are its halves measured alone, which is what makes it readable.

`nolimit` is the same method with the limits taken off. The two exist separately because one
column cannot answer both of the questions asked of it - "what does a player wait for" and
"where does this method actually stop" - and a single column answered neither, running at a
two-second clock on a six-gigabyte manager where a player gets one second and 256 MB
(de-xegj). `ingame` asks the product for its budgets rather than restating them; `nolimit`
states its own, which is the one place that is right, because it is deliberately not the
product's configuration.

MEASURED ON THE HEAVY GROUPS, and the gap is the point: on 631 the in-game column proves
`not-there` in 80 ms holding 176,276 nodes where the unlimited one takes 22.4 seconds and
30.1 million. Both agree; they disagree wildly about what it costs to be sure.

### A column with a forward slice compares on its VERDICT and on nothing else

The verdict is a settled fact about the search and compares between runs. `ms` and `setup`
are clocks and nobody reads them as anything else. Everything in between - `nodes`, `by`,
`asked` - looks like the first and, on a column that runs a forward slice, behaves like the
second.

Measured 2026-09-09 (de-12wr.3, de-12wr.1). The same binary, the same rows, several times:

| column | backward-only arm | `ingame` arm |
|---|---|---|
| `verdict` | identical | identical |
| `nodes` | identical to the node | 154,909 against 160,876 on one row |
| `by`, `asked` | identical | `Forwards asked=0` on three runs of four, `Backwards asked=1` on the fourth |

The difference is the FORWARD SLICE. It is given fifty milliseconds and does as much as fifty
milliseconds of that machine buys - so on a row where the slice is close to answering, whether
it gets there is a property of the machine. The ANSWER does not change: a row that flips to
`Backwards` has the driver finish what the slice did not, and reports the same verdict.

So two folders whose `ingame_by`, `ingame_asked` or `ingame_nodes` differ differ about the
machine, and two whose `bwd_*` differ differ about the search. Only the second is a finding.
**A driver change, an index change or a refactor is checked on verdicts**, which is what
`tools/matrix-compare.py` reports first and why it reports it separately.

It used to move on every column, for a reason that was not inherent: the group graph yielded
its entries in hash-map order, seeded per process, so the backward work followed a different
order in every run - 126,106, 126,588 and 126,148 on one row across three processes, one of
which overflowed a stack the others did not. `LookAheadGraph::nodes()` is ordered now, and
**a run from before that change cannot have its `nodes` column compared with one after it**,
on any column.

**READING AN OLDER RUN, and the names have moved three times.**

| the columns say | what they were |
|---|---|
| `fwd`, `bwd`, `ingame`, `nolimit`, with a `_setup` beside each `_ms` | today's, all symbolic |
| the same four without any `_setup` | before de-x8ms.1. The `_ms` column means the same thing it does now - the whole of what the engine took - so those rows read straight across; what is missing is how much of it was not searching, which on a floor row is nearly all of it |
| `fwd`, `bwd`, `fwdbwd` | one portfolio column instead of two, at neither the player's settings nor a real no-limit. `fwdbwd` reads closest to today's `nolimit` in its clock and to neither in its memory - it ran two seconds on a six-gigabyte manager. Do not read it as `ingame` (de-xegj) |
| `explicit`, `symfwd`, `symbwd` | older names. `symfwd` is today's `fwd` and `symbwd` today's `bwd`; `explicit` measured a state-at-a-time search, which nothing measures now |
| `fwd`, `bwd` and nothing else | older still. `fwd` is the state-at-a-time search and `bwd` is today's `fwd`; no backward search was measured at all |

So a `fwd` column means opposite things at the two ends of that table, and the way to tell
is what else is in the header. `tools/measure-matrix.py`'s `RowWeights` decides it the same
way when it reuses old rows as weights.

Not every run holds all three. `ENGINES` narrows the selection and the header follows it,
so a narrowed run is a narrower row rather than a wide one with holes; read the header
rather than assuming the columns.

Every engine in a run gets the same allowance, which is the only way the verdicts mean
anything against each other: the shared measurement budget in `DiagramBudget::measurement()`,
plus a time cap that is meant not to be what stops a row. See de-e23q and de-z5sp.

### `no-room`, `CRASHED`, `NO-ROWS`, `not-worth-hunting` and `NOT-MEASURED` differ

They look alike from outside - the row has no numbers in it - and they mean opposite
things, so the run keeps them apart.

- `no-room` is a RESULT, and at six gigabytes an interesting one: the search was given
  every byte it was allowed and still had no answer.
- `CRASHED` is a result too. The row took its process down; the log says how, and that is
  a fact about the search.
- `NO-ROWS` is a result about the GROUP rather than the search: no group builds from this
  start, or it has no entry 0, or nothing is reachable from it, so there was never
  anything to measure. It matters at whole-game scale, where plenty of groups are like
  this and reading them as `CRASHED` would fill a run with alarming rows that only mean
  "no dialogue here". SINCE de-cziy IT IS NOT WRITTEN ANY MORE - such groups are skipped
  and counted, and naming one on the command line stops the run - but folders recorded
  before that hold these rows and are read as they always were.
- `not-worth-hunting` appears in the portfolio columns only, and is a result about the
  QUESTION rather than the search: nothing link-reachable outranks where the option already
  lands, so the game refuses to search and answers completely without doing any work. The
  column exists to be what the game runs, so it has to refuse where the game refuses
  (de-qh27). Do not read it as `not-there`: that one means a search ran and found nothing,
  and telling the two apart is the whole reason it has its own word.
- `NOT-MEASURED` is not a result at all. The machine could not supply the budget, so
  nothing ran and there is nothing to learn - the row wants running again when the memory
  is free. A run holding any of these is not yet a measurement, and the script says so at
  the end rather than leaving it to be noticed.

The distinction needs asking BEFORE the memory is spent, because spending it has no
failure path: the diagram manager preallocates its node store with `Vec::with_capacity`,
which aborts the process rather than returning an error. `DiagramBudget::can_be_supplied`
reserves the same bytes fallibly first.

## Regenerating

    tools/measure-matrix.sh              # the six heavy conversations
    tools/measure-matrix.sh 368 631      # just these

One row per process, because a row can take the process down with it - see the script.
Expect the better part of an hour for the whole set: the heavy groups spend the full time
cap on several rows.

### The whole game, resumably

    DEGCT_MATRIX_OUT=measurements/logs/whole-game tools/measure-matrix.sh all

`all` asks the measurement which groups exist - `DEGCT_GROUPS_ONLY=1`, one canonical start per
distinct closure, heaviest first - so nothing decides what is in the run except the index.
It is 1,422 groups against the six a default run does, and it is a run of days rather than
of an hour.

The same enumeration says how many entries each group can reach from its start, and 901 of
the 1,422 reach none - nearly all of them the two-entry `ORB` stubs the database is full
of. Those are recorded as `NO-ROWS` straight from the enumeration, which answers for the
whole game in about a third of a second, rather than by 9,010 processes that each read the
index, build the same graph and find the same nothing. The folder still gets a TSV per
group with a row per profile, so nothing downstream can tell the difference; the reason
for each is in `groups.log` beside them. It is asked for and not cached, for the same
reason the group list is: a committed list of empty groups is a second copy of the index's
shape, and it would be wrong and silent the first time a group grew an entry.

### Where the run stops measuring one group at a time

The heavy groups are measured one at a time and the tail several at once, and the run
decides where that is from what it has just measured rather than from a number written
down. It switches when ten groups in a row have both settled within twice the cheapest
group the run has seen and held at most half the nodes a parallel worker's share of the
budget buys.

Both halves matter. A group measured in parallel gets a DIVIDED budget - each worker is
allowed `6144/WORKERS` MB, because the manager preallocates two thirds of its allowance up
front and four of them at the full budget would commit four times it - so a group that
would not have fitted that share produces a `no-room` that says the run rationed it rather
than that the search ran out. And its clock is contended, which is the other way a row
stops being comparable with one measured alone.

Ten in a row rather than one, because the cost curve is not monotone: over the whole game
the ten groups after the seven heavy ones look exactly like the tail, and then 825, 362,
1030 and 625 arrive - the last of them 47s and a gigabyte, at group 26. A window of one
hands all four to the workers. Ten does not switch until group 35, after which the
heaviest group left in the game is 15s and 33 MB, or two per cent of a worker's cap.

`SETTLE_GROUPS`, `SETTLE_FACTOR` and `MEMORY_HEADROOM` move the rule; `DEGCT_SERIAL_GROUPS=n`
replaces it with a fixed count, which is how a run that has to be comparable with an
existing folder asks for one; `DEGCT_WORKERS=1` never switches at all.

`MATRIX_OUT` names the folder instead of generating one, and THAT is the resume: run the
same command again after a kill, a crash or a reboot and every row already in that folder
is skipped. There is no separate resume mode to remember and no flag to forget. Rows are
appended as they finish, so an interruption costs the row in flight and nothing else; a
retried `NOT-MEASURED` row leaves both lines, and the LAST row for a (conv, profile) is the
one to read.
