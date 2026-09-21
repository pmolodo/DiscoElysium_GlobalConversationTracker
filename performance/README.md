# Performance

Under `logs/`, what the long-running measurements produced. Nothing under here is committed
except this file.

The measurements are `crates/gct-measure`: its `examples/` are the drivers, one file each,
and its `src/` the modules they share.

The directory is named for what the measurements are ABOUT rather than for what they are,
which keeps it beside `testing/` and `analysis/` as one of three things a run can be: a
number to compare, a suite that passes or fails, or a reading of what a measurement wrote.
`tools/run-logged.sh --kind` picks between their log trees.

## Why they are not in `tests/`

A test passes or fails on its own; a measurement produces a number a person reads. The two
were being confused - this repository has already had a measurement's figures quoted as
though a test had verified them - and a measurement that asserts nothing still looks green
when it is run, which is what invites that mistake. See de-18bo.

They are Cargo EXAMPLES, and of a package the root one takes only as a dev-dependency, so
`cargo test` compiles and links none of them on its way to running none of them. Each still
links a thirty-megabyte library, and there are thirty-two.

**WHICH MEANS THE SUITE DOES NOT COMPILE THEM, and the way they break is a shared module
changing under them.** One command checks every driver without running any:

    cargo build --release -p gct_measure --examples

Run it before committing anything in `crates/gct-measure/src/` or in the library's public
surface. Nothing else compiles a driver, so a driver stays broken until someone reaches for
it - which is months, for the ones reached for once a question.

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
  `--out`, to be run on both sides of a parser change and diffed. Identical output
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

    tools/run-logged.sh cargo residue -- cargo run --release -p gct_measure --example search_residue

    tools/run-logged.sh cargo slots -- \
      cargo run --release -p gct_measure --example layout_shape -- slots --conversation 14

Several take an argument, and it selects a stage rather than a setting. `layout_shape`
takes `groups` (the default), which counts the slot classes per group, or `slots`, which
lists one group's slots so the classifier behind the counts can be read rather than trusted.
Each such driver's module doc lists its own stages.

## The whole-game menu matrix

`menu_matrix.rs` is the measurement a whole-game run is taken with: one group per row, a
whole response menu marked against one manager the way the product marks it. Its module doc
says what the columns mean and which settings it reads.

`tools/measure-menus.py` drives it, ONE GROUP PER PROCESS, so a group that takes its process
down costs that group and nothing else:

    tools/measure-menus.py 368 631                        # just these
    tools/measure-menus.py --workers 1 all                # every group, one at a time
    tools/measure-menus.py --out performance/logs/whole-game all
    tools/measure-menus.py --workers 1 --runs 3 all       # three runs, combined

`--runs N` takes N runs back to back, each in `run-1` ... `run-N` under the run's folder, and
when N is more than one writes `combined.tsv` - each group's median, min and max `menu_ms` and
whether its outcome agreed across runs - and `summary.txt`, the per-run totals and the
costliest groups, beside them.

`all` asks `group_list` which groups are worth measuring - one canonical start per distinct
group, most reachable first - so nothing decides what is in the run except the index. A group
that reaches nothing from its start is not in that list, and neither is one already known to
have no menu: both are answers to the same question, and neither is a measurement waiting to be
taken. It is a command of its own rather than a mode of the measurement, because enumerating the
game and timing a menu in it are different jobs, and its answer is kept - transparently, the way
a memo is, except on disk.

The heavy groups are measured one at a time and the rest several at once, and the driver
decides where that switch is from what it has just measured; see the driver's module doc and
`Settling` in `tools/measurement_common.py`.

`--out` names the folder instead of generating one, and THAT is the resume: run the
same command again after a kill, a crash or a reboot and every group already in that folder
is skipped. Rows are appended as they finish, so an interruption costs the group in flight and
nothing else.

A PATH places the folder; ONE PLAIN WORD labels it - `--out qy5t-before` - and the
label rides as a suffix on the name the run would have had anyway,
`..._measure-menus_menus__qy5t-before/`, so the folder still carries its transcript's name.
The same word later resumes the most recent folder carrying it.

### What a run keeps, so it is not derived 521 times

Reading the index, building a group's graph and building its world from the save are the same
answers in every process of every pass, and they were most of what a pass spent. They are now
kept under the build output - `target/degct-cache/`, never in the repository - and read back;
see `crates/gct-measure/src/kept.rs` and `crates/gct-measure/src/prepared.rs`, and `index ms`
for what a process that needed none of them reports.

What `group_list` answers is kept as well - what each group reaches, and whether anything it
reaches offers the player a choice - and that one is keyed on the CONVERSATIONS rather than on
the code, because it is a fact about the dialogue: the engine reads it, it does not decide what
is in it. So it survives a rebuild, and a regenerated index that says the same thing keeps it.
The cost of such a key is that a change to how the fact is DERIVED goes unnoticed by it, which
is what `prepared::DERIVATION` is for: bump it when the meaning changes.

The values that ARE the code's - the parsed index, a group's graph - carry it in their key, so a
kept value cannot outlive the code that derived it: a rebuild costs one pass at full price and
the passes after it are the cheap ones. `--no-cache` derives everything, and `--cache-verify`
derives everything AND checks it against what was kept, which is what `tests/kept_cache.rs`
runs over a handful of groups.

A default row is the shipped algorithm, walk included: each profile menu is asked with the walk
a player would have been shown from the conversation's start (`hub::walk_to_menu`), and the hub
cut it drives is worked out inside the timing, as a player's request has it worked out. Every
driver that asks menus - `cache_split_menu`, `manager_reuse`, `menu_residue`, `menu_wall`,
`nodes_repeat`, `workspace_menus` - asks with a walk the same way (de-r2xf.11).
Every `--marking` arm asks the onward question first, because the product always does. There
is no arm that puts the exact marking on every group: no state exists in which a player's engine
marks a menu without asking the cheap question first, so such a row would describe code the game
cannot run. A test that wants the exact answer calls `menu::mark_menu` directly.

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
- `NOT-MEASURED` is not a result at all. The machine could not supply the budget, so nothing
  ran and there is nothing to learn - the group is taken again by the next run pointed at the
  same folder.

Two things that are NOT rows, and used to share the word `NO-MENU` between them: a group that
contains no menu anywhere it reaches, which is a fact about the dialogue and keeps such a group
out of the list entirely; and a profile this run could not build, which depends on the world it
walked and what it was told to treat as unread, and is said on stderr while nothing is written.

That last distinction has to be asked BEFORE the memory is spent, because spending it has no
failure path: the diagram manager preallocates its node store with `Vec::with_capacity`,
which aborts the process rather than returning an error. `DiagramBudget::can_be_supplied`
reserves the same bytes fallibly first.
