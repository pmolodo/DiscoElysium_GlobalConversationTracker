# Performance

The long-running measurements themselves, and under `logs/` what they produced. Nothing
under here is committed except the measurements and this file.

The directory is named for what the measurements are ABOUT rather than for what they are,
which keeps it beside `testing/` and `analysis/` as one of three things a run can be: a
number to compare, a suite that passes or fails, or a reading of what a measurement wrote.
`tools/run-logged.sh --kind` picks between their log trees.

## Why they are not in `tests/`

A test passes or fails on its own; a measurement produces a number a person reads. The two
were being confused - this repository has already had a measurement's figures quoted as
though a test had verified them - and a measurement that asserts nothing still looks green
when it is run, which is what invites that mistake. See de-18bo.

They are Cargo EXAMPLES, named in `Cargo.toml` with an explicit `path` because Cargo looks
for examples in `examples/` and these live beside their logs. Being examples also takes
them out of `cargo test`, which would otherwise compile and link every one of them on the way
to running none of them.

**Not everything here produces a number**, and the directory's name undersells it. Three of
its contents are a different kind of thing, and they are here because the split that
matters is "runs in the suite" against "run by hand" - a second directory for three files
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
- `greedy_playthrough.rs` is a GENERATOR. It walks each group the way a player would - always
  to the nearest entry not yet shown, counted in presses - and writes the walk under
  `analysis/playthroughs/`: the keypresses, the menu on screen at each decision, and the flat
  node walk. The keys are what the in-game harness presses; they are NOT something
  `walkthrough::walk_inputs` will take back, because that function must finish at a menu and a
  session finishes wherever its last leg's target was - see `sessions_replay`, which measured
  that and found none of them accepted. NOT COMMITTED, for the reason the group partition is not - it is
  derived from the index and goes stale silently the first time a group grows an entry.
  `menu_profile::walked_profile` builds the same walk for itself rather than reading the file,
  so nothing breaks when it is missing; the file is for reading, and for anything that wants a
  playthrough without computing one.

What they have in common with the measurements is the thing that got them moved: none of
them passes or fails on its own, so none of them belongs in `tests/`.

Run one the way anything slow is run here - through `tools/run-logged.sh`, so there is a
log to read afterwards:

    tools/run-logged.sh cargo answers -- cargo run --release --example symbolic_answers

    DEGCT_CONVERSATION=14 \
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

## The whole-game menu matrix

`menu_matrix.rs` is the measurement a whole-game run is taken with: one group per row, a
whole response menu marked against one manager the way the product marks it. Its module doc
says what the columns mean and which settings it reads.

`tools/measure-menus.py` drives it, ONE GROUP PER PROCESS, so a group that takes its process
down costs that group and nothing else:

    tools/measure-menus.py 368 631                        # just these
    DEGCT_WORKERS=1 tools/measure-menus.py all            # every group, one at a time
    DEGCT_MENUS_OUT=performance/logs/whole-game tools/measure-menus.py all
    DEGCT_WORKERS=1 tools/measure-menus.py --runs 3 all   # three runs, combined

`--runs N` takes N runs back to back, each in `run-1` ... `run-N` under the run's folder, and
when N is more than one writes `combined.tsv` - each group's median, min and max `menu_ms` and
whether its outcome agreed across runs - and `summary.txt`, the per-run totals and the
costliest groups, beside them.

`all` asks the measurement which groups exist - `DEGCT_GROUPS_ONLY=1`, one canonical start per
distinct group, most reachable first - so nothing decides what is in the run except the
index. The groups that reach nothing from their start are skipped from that list rather than
by a process each.

The heavy groups are measured one at a time and the rest several at once, and the driver
decides where that switch is from what it has just measured; see the driver's module doc and
`Settling` in `tools/measurement_common.py`.

`DEGCT_MENUS_OUT` names the folder instead of generating one, and THAT is the resume: run the
same command again after a kill, a crash or a reboot and every group already in that folder
is skipped. Rows are appended as they finish, so an interruption costs the group in flight and
nothing else.

A PATH places the folder; ONE PLAIN WORD labels it - `DEGCT_MENUS_OUT=qy5t-before` - and the
label rides as a suffix on the name the run would have had anyway,
`..._measure-menus_menus__qy5t-before/`, so the folder still carries its transcript's name.
The same word later resumes the most recent folder carrying it.

### What a run keeps, so it is not derived 521 times

Reading the index, building a group's graph and building its world from the save are the same
answers in every process of every pass, and they were most of what a pass spent. They are now
kept under the build output - `target/degct-cache/`, never in the repository - and read back;
see `performance/kept.rs` and `performance/prepared.rs`, and `index ms` for what a process that
needed none of them reports.

Every key carries the executable and the files the value came from, so a kept value cannot
outlive the code that derived it: a rebuild costs one pass at full price and the passes after
it are the cheap ones. `DEGCT_NO_CACHE=1` derives everything, and `DEGCT_CACHE_VERIFY=1`
derives everything AND checks it against what was kept, which is what `tests/kept_cache.rs`
runs over a handful of groups.

A default row is the shipped algorithm, walk included: each profile menu is asked with the walk
a player would have been shown from the conversation's start (`hub::walk_to_menu`), and the hub
cut it drives is worked out inside the timing, as a player's request has it worked out. Every
driver that asks menus - `cache_split_menu`, `manager_reuse`, `menu_residue`, `menu_wall`,
`nodes_repeat`, `workspace_menus` - asks with a walk the same way (de-r2xf.11).
`DEGCT_MARKING=bnb` puts the exact marking on every group instead of the shipped hybrid, as an
opt-in comparison, so two row files can be compared on the same profile and the same allowance.

## Where a run's results are

Under `logs/`, one folder per run, holding the rows, the group list the run was built from,
and what the groups said on stderr:

    logs/2026-09-13_menus-shipped-c74337d-1/
        menus.tsv        <- one row per group
        groups.tsv       <- the group list this run measured
        menus.log        <- each group's stderr, headed by its conversation

A folder the driver names itself is named the way every run log in this repo is named - the
date, the commit it measured, the tool and the run - and carries `-dirty` when the tree had
been changed since that commit. See "Run logs" in DEVELOPING.md.

`tools/measure-symbolic.sh` writes here the same way, a folder per run named for the
measurement it ran, and runs a symbolic measurement ONE CONVERSATION PER PROCESS:

    tools/measure-symbolic.sh backward_support 368 631
    tools/measure-symbolic.sh layout_shape slots 14

The first argument is the example's name. A measurement with several stages behind one
`main` takes the stage second; anything that looks like a number is read as a conversation,
so the stage can be left out.

## Why nothing here is committed

A row is a wall-clock time on one machine, and it moves whenever anything about the search
moves - the memory allowance, the width of a state, which machine ran it, what else that
machine was doing. A committed baseline like that is wrong far more often than it is right,
and wrong SILENTLY, because nothing re-runs it to find out. A reader who trusts it is worse off
than one who has nothing.

What stands in for one is comparing two runs: two folders, each holding its own rows next to
the logs those rows came from, each stamped with the commit it measured. That is the honest
shape of the comparison, and it makes the question "against what?" impossible to skip.

`tools/menu-regressions.py` keeps that shape for regression checks. A person marks a clean,
several-run menu measurement of the shipped algorithm as a baseline, which copies it under
`logs/baselines/` - so it is still one machine's and still not committed - and a later run is
checked against the newest baseline measured under the same settings, algorithm and hardware,
and refused where there is none.

## A row that is not a measurement

Several verdicts look alike from outside - the row has no numbers in it - and they mean
opposite things, so the run keeps them apart:

- `CRASHED` is a RESULT. The group took its process down; its stderr in `menus.log` says how,
  and that is a fact about the search.
- `NO-MENU` is a result about the GROUP: no start of it has anything worth hunting beyond it,
  so there was never a menu to mark.
- `NOT-MEASURED` is not a result at all. The machine could not supply the budget, so nothing
  ran and there is nothing to learn - the group is taken again by the next run pointed at the
  same folder.

That last distinction has to be asked BEFORE the memory is spent, because spending it has no
failure path: the diagram manager preallocates its node store with `Vec::with_capacity`,
which aborts the process rather than returning an error. `DiagramBudget::can_be_supplied`
reserves the same bytes fallibly first.
