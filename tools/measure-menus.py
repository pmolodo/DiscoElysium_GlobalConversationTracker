#!/usr/bin/env python
# run-log-kind: performance

"""Measure a whole MENU per group, one group per process: the heavy groups one at a time, then
the rest several at a time.

A request is a whole response menu answered against one manager, so the menu is what a player
waits for and it is not the sum of its options. See crates/gct-measure/examples/menu_matrix.rs for what the
columns mean.

Usage:
    tools/measure-menus.py [conversation ...|all]

Examples:
    tools/measure-menus.py 368 631      # just these two
    tools/measure-menus.py all          # every group in the game, resumably
    tools/measure-menus.py --workers 1 all   # every group one at a time
    tools/measure-menus.py --runs 3 --out base all \
        --arm shipped-defaults --arm first-link --menu first --targets link-deepest
                                        # two arms, one build, a folder each: base-shipped-defaults
                                        # and base-first-link; lines read [Arm 1/2][Run 1/4][Conv ...]
                                        # everything after the first --arm is the arms'

ONE PROCESS PER GROUP for the reason the other drivers give: a group can take its process
down - conversation 28's deepest entries overflow the stack inside a recursive diagram
operation - and with every group in one process the first crash destroys every group after
it. A crash here is a RESULT for that group, recorded as CRASHED, and costs nothing else.

`all` ASKS `crates/gct-measure/examples/group_list.rs` WHICH GROUPS ARE WORTH MEASURING rather than keeping a
list here, so what is in a run is decided by the index and nothing else. The enumeration
arrives heaviest-first, which is what the serial phase below rests on.

THE HEAVY GROUPS ARE MEASURED ONE AT A TIME, because workers and timings pull opposite ways:
groups in parallel finish the run several times sooner and make every millisecond column a
measurement of how busy the machine was - and the heavy groups are the ones whose milliseconds
anybody reads. So the run measures groups one at a time, heaviest first, until the cost has
bottomed out, and only then hands the rest to `--workers` at once. The rule lives in
`measurement_common.Settling`: `--settle-groups` settled groups in a row, a group counting as
settled when it is within `--settle-factor` of the cheapest menu so far or under `--settle-ms`
outright. At least `--settle-groups` groups are always measured one at a time. `--help` gives the
defaults.

WHAT COUNTS TOWARDS SETTLING. A measured menu counts by its `menu_ms`. A CRASHED or
NOT-MEASURED group resets the count, since it is evidence that something did not measure
rather than that measuring got cheap. A NO-MENU group neither counts nor resets: no start of it
had anything worth hunting, which says nothing about what the next menu costs.

RESUMING. Rows are written as they finish, and pointing a later run at the same folder makes
it skip the groups already there:

    tools/measure-menus.py --out performance/logs/2026-09-09/menus all

The same command is the start and the resume; there is no separate mode to remember. A resumed
group still counts towards settling, by the row it left, so a resume switches where the
original run would have. Without `--out` each run gets its own folder and resumes nothing.

SEVERAL RUNS. `--runs N` takes N runs of the same groups back to back, each in run-1 ... run-N
under the one folder, because a single run's milliseconds are a reading of the machine as much
as of the search. It then writes combined.tsv - each group's median, min and max menu_ms, its
median nodes, and its rounds, settled and starred with a flag for whether every run agreed -
and summary.txt, the per-run totals and the costliest groups, and prints the summary:

    tools/measure-menus.py --workers 1 --runs 3 all

The summary also says which groups' nodes moved between the runs, and by how much, since that
column moves a little without the search moving at all. See NODES_NOISE.

A THROW-AWAY RUN COMES FIRST WHERE TIMING IS WHAT IS BEING MEASURED, so --runs 3 takes four and
--runs 1 takes two. It goes in run-cold, is reported beside the combination and left out of it,
and is kept on disk rather than deleted. See COLD_FOLDER for the evidence, and
`tools/cold-run-effect.py` to recompute it.

IT IS A TAX ON TIMINGS ONLY. This is a performance tool, and asking it for a DATASET rather than
a number is an ordinary thing to want - which groups fall through to step 2, say, via
`--marking onward`. Those columns read the same cold as warm, so say what the run measures and
skip the extra pass:

    tools/measure-menus.py --kind analysis --runs 1 all

--kind overrides the DEGCT_RUN_KIND that `tools/run-logged.sh --kind` exports, and with neither
the run counts as a performance run and pays. See `takes_cold_run`.

AND IT PLACES THE ROWS, since a dataset filed among the timings is one that later readings of
performance/logs take for a measurement: the folder of rows goes under the tree the kind names,
beside the transcript of a run wrapped as the same kind. See `measurement_common.in_tree`.

WHAT COUNTS AS DONE:

    a row       measured, whatever it says. Done.
    CRASHED     the group took its process down. That IS the answer for that group.
    NOT-MEASURED  the machine could not supply the budget. NOT done - it is re-run.

WHAT IS NEVER ASKED ABOUT. A group with nothing to measure is not in the run at all. Measured
2026-09-19: of the game's 1,422 conversations, 901 reach nothing from their start, and 92 of the
521 groups that remain contain no menu anywhere they reach. Both are facts about the DIALOGUE,
both are answers to the one question `group_list` answers, and both are kept - so the game is
enumerated once rather than in every run.

WHAT A RUN STILL MEETS is a group it cannot build its profile in: that depends on the world it
walks into and how many of the deepest entries it was told to treat as unread, so it is a
finding rather than a fact. The measurement says so on stderr and writes no row. 130 of the 429
under the current default. A run therefore holds only rows that are measurements. See de-ealo.
"""

import argparse
import re
import shlex
import statistics
import sys

from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import measurement_common as common  # noqa: E402  (after the path is set)

from measurement_common import (  # noqa: E402
    COMBINED,
    KINDS,
    PERFORMANCE_KIND,
    SUMMARY,
    TAB,
    Settling,
    build_measurement,
    default_workers,
    progress_line,
    refuse,
    run_groups,
)

###############################################################################
# Core functions
###############################################################################

# The example this drives, and the one it asks which groups are worth measuring. Two commands
# because they are two jobs: `menu_matrix` only ever measures a menu, and `group_list` only ever
# enumerates. See `crates/gct-measure/examples/group_list.rs`.
MENUS = "menu_matrix"
GROUPS = "group_list"

# A verdict that means the row was never taken, so a resume takes it again.
RETRY = "NOT-MEASURED"

# The verdict for a group whose process died, written by this driver.
CRASHED = "CRASHED"


# The column a menu's cost is in, and where a verdict goes instead for an unmeasured group.
MENU_MS = "menu_ms"

# Where that column sits in a row, for reading one off a row as it is reaped - fifth, after
# conv, entries, options and offered. Only for a progress line: everything that has to be
# right reads the header by name instead, because the matrix's columns have moved before.
MENU_MS_COLUMN = 4

# The absolute arm of the settle rule for a menu. Menus bottom out near twenty milliseconds - a
# matrix group, many rows long, near two seconds - so a menu under this is at the floor whatever
# the relative arm says of it.
SETTLE_MS = 100

# What this driver calls the option that names its folder, said in the refusal when there is no
# bash to ask the wrapper for a name. See `measurement_common.run_folder`.
OUT_OPTION = "--out"


def driver_arguments(marking, driver):
    """What every group's process is asked, beyond which group it is.

    THE DRIVER OWNS ITS OWN OPTIONS, which is why everything but the arm is passed through
    unread: this tool would otherwise carry a copy of `menu_matrix`'s interface, and a copy is a
    thing to keep in step. A misspelt option is refused by the driver, on the first group.

    THE ARM IS NAMED SEPARATELY because it is the one option this tool has to understand: a run
    record says which algorithm it measured, and `measurement_common.algorithm_of` reads it back
    to decide whether two folders may be compared and whether a run can stand as a baseline.
    """
    return [*(["--marking", marking] if marking else []), *driver]


class Run:
    """One folder of rows, and what has already been written into it."""

    def __init__(self, out, menus, digest, marking=None, driver=()):
        # HELD RATHER THAN REBUILT where it is needed, so every group in a run is measured under
        # the same arm by construction.
        self.asked = driver_arguments(marking, driver)
        self.folder = Path(out)
        self.folder.mkdir(parents=True, exist_ok=True)
        self.rows = self.folder / "menus.tsv"
        self.log = self.folder / "menus.log"
        # THE BINARY IS HANDED IN, BUILT ONCE FOR THE WHOLE PASS, AND CHECKED HERE. Building
        # once makes the runs agree only as far as this process is concerned; anything else on
        # the machine can relink the file between run 2 and run 3. Refused rather than reported,
        # because the rows either side would be a comparison between two binaries wearing one
        # revision - see `measurement_common.binary_digest`.
        self.menus = menus
        now = common.binary_digest(menus)
        if now != digest:
            refuse(
                f"{Path(menus).name} changed during this pass - built {digest[:12]}, now "
                f"{now[:12]}.\n"
                "Something rebuilt it between runs: most likely a cargo build started by hand "
                "while this was running.\n"
                f"The runs already in {self.folder.parent} measured the earlier binary, so the "
                "pass is half one build and half another. Re-run it.",
            )

    def recorded(self):
        """The rows already written, by conversation, so a resume can skip them.

        A ROW THAT SAYS NOT-MEASURED IS NOT DONE, because the machine could not supply the
        budget and nothing about the menu was learned. Every other row is an answer, a crash
        included.
        """
        if not self.rows.exists():
            return {}
        finished = {}
        for line in self.rows.read_text(encoding="utf-8", errors="replace").splitlines()[1:]:
            cells = line.split(TAB)
            if len(cells) < 2 or not cells[0].isdigit():
                continue
            if RETRY in cells:
                continue
            finished[int(cells[0])] = cells
        return finished

    def header(self):
        """Writes the column names, asked of the measurement rather than written here."""
        if self.rows.exists():
            return
        answer = common.ask(self.menus, {}, args=["--header"])
        common.write_lf(self.rows, answer.stdout)

    def columns(self):
        """The column names, off the header this run's rows were written under."""
        return self.rows.read_text(encoding="utf-8", errors="replace").splitlines()[0].split(TAB)

    def groups(self, groups):
        """Every group worth measuring, heaviest first, asked of `groups`.

        WHAT IS WORTH MEASURING IS THE MEASUREMENT'S TO DECIDE, and it answers in one list: a
        group that reaches nothing from its start is not in it, and neither is one already known
        to have no menu under these settings. Neither is a measurement waiting to be taken, and a
        driver that spawned a process for them would be asking a question whose answer is already
        written down. See `crates/gct-measure/examples/group_list.rs`, which also says why it is a command of its
        own rather than a mode of the measurement.
        """
        answer = common.ask(groups, {})
        if answer.returncode != 0:
            refuse(f"the group enumeration failed:\n{answer.stderr}", code=1)

        common.write_lf(self.folder / "groups.tsv", answer.stdout)

        wanted = []
        for line in answer.stdout.splitlines():
            cells = line.split(TAB)
            if len(cells) < 4 or not cells[0].lstrip("-").isdigit():
                continue
            wanted.append(int(cells[0]))
        return wanted

    def measure(self, conversation):
        """One group, in a process of its own."""
        answer = common.ask(
            self.menus,
            {},
            args=["--conversation", conversation, *self.asked],
        )
        if answer.returncode != 0 and not answer.stdout.strip():
            # A CRASH IS THIS GROUP'S ANSWER, written as a row so a resume does not take it
            # again and a reader can see which groups the engine cannot survive.
            return f"{conversation}\t{CRASHED}\n", answer.stderr
        return answer.stdout, answer.stderr

    def append(self, rows):
        with common.open_lf(self.rows) as handle:
            handle.write(rows)


def menu_cost(cells, columns):
    """What one group's row says about its cost: (ms, complete).

    See the module doc for why a crash resets the settle count. A group this run could not build
    a profile for leaves no row at all, so it never reaches here: it neither counts towards
    settling nor resets it, which is what it always meant.
    """
    if CRASHED in cells:
        return 0, False
    at = columns.index(MENU_MS)
    if at >= len(cells):
        return 0, False
    cell = cells[at]
    if not cell.isdigit():
        return 0, False
    return int(cell), True


def serial_phase(run, conversations, recorded, workers, settle, reap):
    """The heavy groups one at a time, until their cost has bottomed out.

    Returns how many of `conversations`, in order, the phase covered. A group already recorded
    is not measured again, but its row still counts towards settling.

    `settle` is the RULE, and this pass watches with a fresh copy of it - see `Settling.fresh`.
    """
    settle = settle.fresh()
    columns = run.columns()
    for position, conversation in enumerate(conversations, start=1):
        if conversation in recorded:
            cells = recorded[conversation]
        else:
            rows, errors = run.measure(conversation)
            reap(conversation, (rows, errors))
            if not rows.strip():
                # NO ROW AND NO CRASH: this run could not build a profile here, so nothing was
                # measured and nothing failed. It neither counts towards settling nor resets it,
                # which is what a group with nothing to measure has always done.
                continue
            cells = rows.splitlines()[0].split(TAB)

        if workers <= 1:
            continue

        menu_ms, complete = menu_cost(cells, columns)
        if not complete:
            settle.reset()
            continue

        if settle.observe(menu_ms):
            print(
                f"  cost has bottomed out after {position} group(s): the last {settle.groups} "
                f"took at most {settle.window_max_ms}ms against a floor of {settle.floor_ms}ms."
            )
            print(f"  The rest go {workers} at a time.")
            return position

    return len(conversations)


def cost_of(rows):
    """What a just-measured group cost, for its progress line, or its verdict.

    READ OFF THE ROW rather than timed here, because the row already carries what the
    measurement itself says the menu cost - and a duration taken around the subprocess would
    include the process launch, which is the driver's overhead and not the menu's.

    Defensive about the shape: a CRASHED or NO-MENU row has a verdict where the milliseconds
    go, and a progress line is not worth an exception.
    """
    cells = rows.strip().split(TAB)
    if len(cells) < 2:
        # NO ROW AT ALL is what a group with no menu leaves: it is not a measurement, so it is
        # not written as one, and the next run will not ask about it - see `Run.groups`.
        return "nothing to measure"
    verdict = cells[MENU_MS_COLUMN] if len(cells) > MENU_MS_COLUMN else ""
    return f"{verdict} ms" if verdict.isdigit() else verdict


def measure(out, conversations, workers, settle, menus, digest, groups, marking, driver, prefix=""):
    """Measures one run into `out`; `prefix` leads every progress line - see `progress_line`."""
    run = Run(out, menus, digest, marking, driver)
    run.header()

    if conversations == ["all"]:
        conversations = run.groups(groups)
        print(f"{len(conversations)} group(s) to measure")

    recorded = run.recorded()
    already = recorded
    todo = [c for c in conversations if c not in already]
    if recorded:
        print(f"{len(recorded)} already measured in {run.folder}; {len(todo)} to go")

    if not todo:
        print("nothing to do.")
        return 0

    if workers <= 1:
        print(f"{len(todo)} group(s), one at a time -> {run.rows}")
    else:
        print(
            f"{len(todo)} group(s) -> {run.rows}: one at a time until the cost bottoms out "
            f"({settle.rule()}), then {workers} at a time"
        )

    state = {"done": 0}

    def reap(conversation, result):
        rows, errors = result
        if rows.strip():
            run.append(rows)
        state["done"] += 1
        line = progress_line(
            state["done"],
            len(todo),
            f"conversation {conversation}",
            note=cost_of(rows),
            prefix=prefix,
            unit="Conv",
        )
        print(line)
        # THE SAME LINE INTO THIS RUN'S OWN LOG. A multi-run pass interleaves every run into
        # the driver's log, so "how far into THIS run" is answerable there only by scrolling
        # back to where the run began - and the per-run file, which is the natural one to
        # tail, held group names and engine stderr but no position at all.
        #
        # THE ENGINE'S LINES STAY MARKED. A progress line is the driver's; what follows a
        # `=== conversation ===` header is the engine's own stderr, so the two are told apart
        # by shape rather than by guessing.
        with common.open_lf(run.log) as handle:
            handle.write(f"{line}\n")
            if errors.strip():
                handle.write(f"=== {conversation} ===\n{errors}")

    serial_done = serial_phase(run, conversations, already, workers, settle, reap)

    remaining = [c for c in conversations[serial_done:] if c not in already]
    if remaining:
        run_groups(remaining, run.measure, workers, reap)

    print(f"\n{state['done']} group(s) measured -> {run.rows}")
    if run.log.exists():
        print(f"what the groups said on stderr is in {run.log}")
    return 0


# Where each of several runs is written, under the folder the runs share.
RUN_FOLDER = "run-{}"

# Where the throw-away first run is written.
#
# A MEASUREMENT TAKES ONE MORE RUN THAN IT WAS ASKED FOR, and throws the first away, so
# --runs 3 takes four and --runs 1 takes two.
#
# ONLY A MEASUREMENT. The discard protects a comparison between TIMINGS, and a run of any
# other kind produces none: a test asks whether the marking is right, and an analysis pass
# asks what the marking decided - which menus fall through to step 2, say. Both read columns
# that are the same in a cold run as a warm one, so a cold pass there is double the wall clock
# for nothing. `DEGCT_RUN_KIND` carries the kind down from the wrapper; see `takes_cold_run`.
#
# THE EFFECT IS RARE AND LARGE, which is exactly the shape that justifies a fixed discard.
# Measured over the 54 multi-run passes already on disk, treating each one's run-1 as the cold
# run it would have been:
#
#   run-1 slowest of its pass    22 of 54 (40%), against 33% by chance for a three-run pass
#   run-1 over the fastest later run by more than 10%     4 of 54 (7%)
#
# THE DIRECTION IS NOT THE FINDING. 40% against an expected 33% is a z of +1.15 - nothing. Two
# deliberate attempts to provoke a ramp found none either: ten runs in one invocation had run 1
# fourth fastest of ten, and forcing a real recompile first left it second fastest of four. On
# a machine already looping this same work, a first run is like any other.
#
# THE TAIL IS THE FINDING. Three whole-game passes show run-1 exceeding the fastest later run
# by 10.4, 12.3 and 13.1 per cent, against spreads AMONG their own later runs of 0.9, 6.8 and
# 2.6. Those first runs sit several times outside the scatter of their own siblings, so they
# are not noise. The distribution is bimodal: absent in fifty passes and large in three or
# four. A pass in that minority is SILENTLY WRONG rather than visibly noisy, which is what one
# extra run per pass is cheap insurance against.
#
# AND THE MECHANISM IS NOT IN DOUBT, only its frequency. A first run is the one that finds the
# page cache without the index in it, the CPU at its idle clock, and the disk cold. Those are
# properties of the machine rather than a hypothesis about it, so the question was never
# whether a cold run can be slower - it was how often the conditions arise. That makes the 7
# per cent a FLOOR rather than an estimate: nearly every pass in the sample was taken back to
# back with others on a machine already doing this work, which is the condition least likely
# to produce a cold start. Over a long enough series of measurements it bites eventually, and
# the cost of being wrong about one is far more than the run it takes to be sure.
#
# KEPT RATHER THAN DELETED, because it is the honest number for a question the combination
# cannot answer: what a player pays on the FIRST menu of a session, which is the one they
# notice. `tools/cold-run-effect.py` recomputes the figures above over whatever is on disk.
#
# AND IT IS WHAT FILLS THE MEASUREMENT'S OWN CACHE, which is a second reason to take it and a
# reason the discard has to stay where it is. A group's graph and its world are derived once and
# kept - see `crates/gct-measure/src/kept.rs` - so the first pass over the game after a build derives them
# and the rest read them, and a pass whose counted runs were half derived and half read would
# be comparing two different amounts of work. The discarded pass takes the deriving, and every
# counted run is warm BY CONSTRUCTION.
#
# THAT MATTERS TO THE MENU COLUMNS TOO, not only to the preparation ones. Whole game 2026-09-19:
# a warm run measures menus 4 to 15 per cent higher than a deriving one, because workers that no
# longer spend most of their time preparing are in menus at the same moment as each other. Two
# folders taken in different cache states are not comparable, and taking the discard is what
# makes sure they never are.
COLD_FOLDER = "run-cold"

# What this driver calls itself where the wrapper is not there to be asked, and what it
# produced. THE TOOL SLOT IS FOR A TOOL: a folder under performance/logs whose tool field says
# "measure" reads as a kind rather than as whatever wrote it, since a kind is what the three
# trees are named for and no kind is called that.
TOOL = "measure-menus"
VERB = "menus"


def takes_cold_run(asked):
    """Whether this run throws a first pass away, which only a performance run does.

    `asked` is --kind, which `common.run_kind` weighs against the wrapper's DEGCT_RUN_KIND.
    Saying it on the command line is what makes deriving a dataset with a performance tool a
    one-word change rather than a reason to reach for a second driver.

    UNKNOWN COUNTS AS TIMING, deliberately. An unwrapped invocation with no --kind says
    nothing about itself, and the failure directions are not symmetric: skipping the discard
    where it was wanted can silently corrupt a comparison, while taking one where it was not
    wanted costs a run. Pay the run.
    """
    return common.run_kind(asked) == PERFORMANCE_KIND


# How many of the costliest groups the summary lists.
HARDEST = 10

# What a run's outcome is judged steady on: columns that say what the marking decided rather
# than what it cost, so they should not move between runs at all.
OUTCOME = ("rounds", "settled", "starred")

# How far nodes may move between runs of the same group before the difference is a difference.
# Unlike an OUTCOME column, nodes is a cost, and a reading of the manager rather than a count of
# the search - so it moves a little without the search moving at all. A spread under this is
# noise, and a nodes figure quoted to the last digit is quoting the noise too.
NODES_NOISE = 0.01
# How many of the widest-moving groups the summary names.
DRIFTIEST = 5

COMBINED_COLUMNS = [
    "conv",
    "runs",
    "menu_ms_median",
    "menu_ms_min",
    "menu_ms_max",
    "nodes_median",
    "rounds",
    "settled",
    "starred",
    "steady",
]


def read_rows(path):
    """One run's rows, by conversation, each a dict keyed by the run's own header."""
    if not path.exists():
        return {}
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    header = lines[0].split(TAB)
    rows = {}
    for line in lines[1:]:
        cells = line.split(TAB)
        if cells and cells[0].isdigit():
            rows[int(cells[0])] = dict(zip(header, cells))
    return rows


def verdict_of(row):
    """What an unmeasured row says instead of a cost."""
    if CRASHED in row.values():
        return CRASHED
    return row.get(MENU_MS) or "?"


def number_text(value, grouped=False):
    """A median as written: whole where it is whole, never in exponent form, and with thousands
    separators only where a person rather than a table reads it."""
    return f"{value:{',' if grouped else ''}.1f}".removesuffix(".0")


def agreed(rows, column):
    """One value where every run agrees on `column`, or every value seen joined by '|'."""
    return "|".join(sorted({row.get(column, "-") for row in rows}))


def cold_line(cold, totals):
    """What the discarded run cost, and how much warmer the kept ones were.

    REPORTED SO THE DISCARD IS VISIBLE. A number thrown away silently is one nobody can
    argue with, and the size of the gap is the evidence for throwing it away at all.
    """
    if cold is None:
        return "cold run: not taken"
    rows = read_rows(cold / "menus.tsv")
    total = sum(int(row[MENU_MS]) for row in rows.values() if row.get(MENU_MS, "").isdigit())
    if not totals or not total:
        return f"cold run (discarded): {total:,} ms"
    warmest = min(totals)
    return f"cold run (discarded): {total:,} ms, {total / warmest:.2f}x the fastest kept run at {warmest:,} ms"


def nodes_drift_lines(drifting, measured):
    """Which groups read a different nodes every run, and by how much.

    SAID WHERE THE NUMBER IS READ. nodes is quoted to the last digit throughout this
    repository - in issues, in menu-costs-diff.py's output, in notes comparing one branch with
    another - and read as a count of the search, so a group that moves on its own reads as a
    change that is not there. The summary is where a reader meets the figure, so it is where
    the figure says how firm it is. See de-jitt.
    """
    if not drifting:
        return [f"nodes: identical in every run for all {measured} measured group(s)"]
    worst = sorted(drifting, reverse=True)
    noisy = [group for group in worst if group[0] >= NODES_NOISE]
    lines = [
        f"groups whose nodes move between runs: {len(drifting)} of {measured} measured, "
        f"{len(noisy)} of them by {NODES_NOISE:.0%} or more "
        "(a smaller spread than that is not a difference)"
    ]
    for spread, conversation, low, high in worst[:DRIFTIEST]:
        lines.append(f"  {conversation:>6}  {low:,} - {high:,}  ({spread:.1%})")
    return lines


def combine(folders, out, cold=None):
    """Folds several runs of the same groups into one table and a summary, and prints it.

    `cold` is the throw-away first run: reported, never folded in. See COLD_FOLDER.

    A GROUP IS COMBINED ON ITS MEDIAN, with the min and max beside it, because a single run's
    milliseconds are a reading of the machine as much as of the search. Its outcome - rounds,
    settled, starred - is shown as the one value every run agreed on, or every value seen
    joined by '|', and `steady` says which.

    A group any run did not measure - CRASHED, NO-MENU, NOT-MEASURED - is combined on its
    verdicts rather than a cost, since there is no cost to take the median of.
    """
    runs = [read_rows(folder / "menus.tsv") for folder in folders]
    conversations = sorted(set().union(*runs))

    lines = [TAB.join(COMBINED_COLUMNS)]
    measured = []
    unsteady = []
    drifting = []
    for conversation in conversations:
        rows = [run[conversation] for run in runs if conversation in run]
        times = [int(row[MENU_MS]) for row in rows if row.get(MENU_MS, "").isdigit()]
        if len(times) != len(rows):
            verdicts = "/".join(sorted({verdict_of(row) for row in rows}))
            cells = [str(conversation), str(len(rows)), verdicts]
            lines.append(TAB.join(cells + [""] * (len(COMBINED_COLUMNS) - len(cells))))
            continue

        nodes = [int(row["nodes"]) for row in rows if row.get("nodes", "").isdigit()]
        outcome = [agreed(rows, column) for column in OUTCOME]
        steady = all("|" not in value for value in outcome)
        if not steady:
            unsteady.append(conversation)
        # NODES IS A READING OF THE MANAGER, NOT A COUNT OF THE SEARCH - what the diagram
        # manager HOLDS when the menu ends, which depends on when it last collected and so on
        # allocation timing rather than on the search's shape. Most groups read the same every
        # run; a few do not, and the number is quoted as exact all over this repository. See
        # de-jitt.
        if len(nodes) > 1 and min(nodes) != max(nodes):
            drifting.append((max(nodes) / min(nodes) - 1, conversation, min(nodes), max(nodes)))
        median = statistics.median(times)
        nodes_median = statistics.median(nodes) if nodes else ""
        lines.append(
            TAB.join(
                [
                    str(conversation),
                    str(len(rows)),
                    number_text(median),
                    str(min(times)),
                    str(max(times)),
                    number_text(nodes_median) if nodes else "",
                    *outcome,
                    "yes" if steady else "no",
                ]
            )
        )
        measured.append((median, conversation, min(times), max(times), nodes_median, outcome))

    common.write_lf(out / COMBINED, "\n".join(lines) + "\n")

    totals = [sum(int(row[MENU_MS]) for row in run.values() if row.get(MENU_MS, "").isdigit()) for run in runs]
    report = [
        f"{len(runs)} runs over {len(conversations)} group(s), {len(measured)} measured in every run",
        "total menu_ms by run: " + " / ".join(f"{total:,}" for total in totals),
        f"sum of medians: {number_text(sum(m[0] for m in measured), grouped=True)} ms",
        f"groups whose rounds, settled or starred differ between runs: {unsteady or 'none'}",
        *nodes_drift_lines(drifting, len(measured)),
        cold_line(cold, totals),
        "",
        f"the {HARDEST} costliest groups by median menu_ms:",
        f"  {'conv':>6}  {'median':>8}  {'min-max':>13}  {'nodes':>10}  rounds  settled  starred",
    ]
    for median, conversation, low, high, nodes_median, outcome in sorted(measured, reverse=True)[:HARDEST]:
        nodes_text = number_text(nodes_median, grouped=True) if nodes_median != "" else "-"
        report.append(
            f"  {conversation:>6}  {number_text(median, grouped=True):>8}  {f'{low}-{high}':>13}  {nodes_text:>10}  "
            f"{outcome[0]:>6}  {outcome[1]:>7}  {outcome[2]}"
        )
    text = "\n".join(report) + "\n"
    common.write_lf(out / SUMMARY, text)
    print(f"\n{text}")
    print(f"combined rows -> {out / COMBINED}; summary -> {out / SUMMARY}")


###############################################################################
# CLI
###############################################################################


def record_run(out, workers, settle, runs, named, kind, marking, driver):
    """Writes what this run is - see `measurement_common.write_run_record` - into its folder.

    PARALLELISM IS THE PART THAT CHANGES WHAT THE ROWS SAY: groups measured side by side pay a flat
    cost in setup that groups measured alone do not - about ten milliseconds a small group,
    measured 2026-09-14 - so `tools/menu-costs-diff.py` refuses to compare folders that disagree
    about it, and a resume into a folder recorded at other parallelism is refused.
    """
    parallelism = {
        "workers": workers,
        "settle": settle.rule() if workers > 1 else None,
    }
    # WHETHER A COLD RUN WAS TAKEN, readable later without counting folders. The wrapper's
    # DEGCT_RUN_KIND is recorded with every other DEGCT_ variable, but --kind overrides it and
    # an unwrapped run has neither, so the decision itself is written down rather than inferred.
    common.write_run_record(
        out,
        parallelism,
        runs=runs,
        groups=named,
        kind=kind,
        cold=takes_cold_run(kind),
        marking=marking,
        driver=driver or None,
    )


def measure_arm(out, named, workers, settle, menus, digest, groups, args, driver, prefix):
    """Every run of one arm into `out`, the cold one first, and their combination.

    THE COLD RUN IS FIRST AND IS NOT COMBINED, and a run measuring anything but timing does not
    take one at all. See COLD_FOLDER and `takes_cold_run`.

    EVERY PASS IS COUNTED IN ITS PROGRESS LINES, the cold one first: a pass is a pass of waiting
    whether or not its rows are combined, so `[Run 1/4]` is the cold run of three.
    """
    kind = common.run_kind(args.kind)
    passes = args.runs + (1 if takes_cold_run(kind) else 0)

    def measure_pass(folder, number):
        where = f"{prefix}[Run {number}/{passes}]"
        measure(folder, named, workers, settle, menus, digest, groups, args.marking, driver, where)

    cold = None
    if takes_cold_run(kind):
        cold = out / COLD_FOLDER
        print(f"\n=== cold run (discarded from the combination) -> {cold} ===")
        measure_pass(cold, 1)
    else:
        print(f"\n=== {kind}: no cold run, its columns do not time ===")

    folders = []
    for number in range(1, args.runs + 1):
        folder = out / RUN_FOLDER.format(number)
        print(f"\n=== run {number} of {args.runs} -> {folder} ===")
        measure_pass(folder, passes - args.runs + number)
        folders.append(folder)
    combine(folders, out, cold=cold)


# What an arm may be called: a plain word, since it becomes part of a folder name.
ARM_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]*")


# Where the arms begin on a command line, and what separates one from the next.
ARM_OPTION = "--arm"


def split_arms(argv):
    """This tool's own arguments, and the arms after them as (name, driver arguments).

    AN ARM IS EVERYTHING FROM ONE `--arm` TO THE NEXT: its name, then its driver's arguments as
    plain words - `--arm first-link --menu first --targets link-deepest`. Nothing is quoted, so
    nothing has to survive a shell's quoting or a second one's, which is where a string of
    arguments inside one argument goes wrong. It also means the arms come LAST: everything
    before the first `--arm` is this tool's, and nothing after it is.

    One arm of no name and no arguments is what a command line with no `--arm` measures, so
    every run - one arm or several - goes through the same loop.

    ## Taken off the end, by argparse, one arm at a time

    ARGPARSE READS THE `--arm` ITSELF, so every form it accepts is accepted - `--arm name`,
    `--arm=name` - and a missing name is its own refusal. But asked of the whole line it keeps
    only the last `--arm` and runs every arm's arguments together. Asked of a SUFFIX, it can
    answer: the shortest suffix holding an `--arm` is the last arm and nothing else, its name
    and its arguments. So the last arm is taken, the line is cut before it, and the same
    question is asked again until no `--arm` is left - and what is left is this tool's own.
    """
    parser = argparse.ArgumentParser(prog=f"{TOOL} {ARM_OPTION}", add_help=False, allow_abbrev=False)
    parser.add_argument(ARM_OPTION)
    own = list(argv)
    arms = []
    while True:
        for start in range(len(own) - 1, -1, -1):
            found, words = parser.parse_known_args(own[start:])
            if found.arm is not None:
                break
        else:
            break
        name = found.arm
        if not ARM_NAME.fullmatch(name):
            refuse(f"{ARM_OPTION} {name}: an arm's name is a plain word, since it names a folder")
        if any(name == seen for seen, _ in arms):
            refuse(f"{ARM_OPTION} {name}: named twice; every arm needs a folder of its own")
        arms.insert(0, (name, words))
        own = own[:start]
    return own, arms or [(None, [])]


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "conversations",
        nargs="*",
        help="group ids, or 'all' for every group in the game",
    )
    parser.add_argument(
        "--runs",
        type=int,
        default=1,
        help=(
            "how many runs to take back to back; more than one writes each under run-N and "
            "combines them (default: %(default)s)"
        ),
    )
    parser.add_argument(
        "--kind",
        choices=KINDS,
        default=None,
        help=(
            "what this run is for, overriding DEGCT_RUN_KIND; it names the tree the rows go "
            f"under, and anything but '{PERFORMANCE_KIND}' skips the throw-away cold run, so "
            "asking a perf tool for a dataset does not pay for one"
        ),
    )
    parser.add_argument(
        "--workers",
        type=int,
        default=None,
        help=(
            "how many groups to measure at once once the cost has bottomed out; the default is "
            "the cores, or what the free memory affords, whichever is smaller"
        ),
    )
    parser.add_argument(
        "--marking",
        default=None,
        help=(
            "which marking to measure, passed straight to the driver, which is what knows the "
            "arms and refuses a misspelt one; the default is the shipped algorithm"
        ),
    )
    parser.add_argument(
        "--out",
        default="",
        metavar="PATH|LABEL",
        help=(
            "where the rows go, and THAT is the resume: run the same command again and every "
            "group already there is skipped. A path is taken literally; a plain word is a "
            "LABEL, carried as a suffix on the name the run would have had anyway"
        ),
    )
    parser.add_argument(
        "--driver",
        default="",
        metavar="ARGS",
        help=(
            "arguments to pass every group's driver, as one string ATTACHED WITH '=' - "
            "--driver=--no-cache, --driver='--nolimit --starts 24'. The '=' is not optional: "
            "argparse reads a separate value that looks like a single option as an option. "
            "See `menu_matrix --help` for what the driver takes"
        ),
    )
    parser.add_argument(
        ARM_OPTION,
        nargs=argparse.REMAINDER,
        metavar="NAME [ARG ...]",
        help=(
            "measure several arms in one invocation, one after another, and LAST on the line: "
            "each --arm is a name and then that arm's driver arguments, unquoted, up to the next "
            "--arm - --arm shipped-defaults --arm first-link --menu first --targets "
            "link-deepest. Every arm gets its own folder, named by --out and the arm, and every "
            "arm is measured by the same build. --driver still applies to all of them, ahead of "
            "each arm's own arguments"
        ),
    )
    parser.add_argument(
        "--status-prefix",
        default="",
        metavar="TEXT",
        help=(
            "printed as given at the start of every progress line, ahead of this run's own "
            "[Run i/N][Conv done/total] - for a caller running this as one of several passes, "
            "to say which: --status-prefix='[Arm 1/4]'"
        ),
    )
    Settling.add_arguments(parser, SETTLE_MS)
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    common.watchable_output()
    # THE ARMS ARE SPLIT OFF BEFORE ARGPARSE SEES THE LINE, since what follows an arm's name is
    # its driver's options, which argparse would read as this tool's. `--arm` stays in the
    # parser for its help alone.
    argv, arms = split_arms(argv)
    args = get_parser().parse_args(argv)
    if args.runs < 1:
        refuse(f"--runs {args.runs}: a run count is at least 1")

    named = args.conversations or ["all"]
    if named != ["all"]:
        named = [int(c) for c in named]

    # NAMED, LABELLED, OR A FOLDER OF ITS OWN. A path is taken literally and resumes what is
    # there; a plain word is a LABEL, which says what the run was for and is carried as a suffix
    # on the name every log here has - see `common.folder_for`. A run told nothing gets a fresh
    # folder and resumes nothing, which is the safe default: a resume into a folder taken
    # against different settings would mix two measurements.
    #
    # THE KIND PLACES IT, transcript and rows alike: a dataset derived by this tool is filed
    # with the datasets rather than among the timings.
    kind = common.run_kind(args.kind)
    #
    # AN ARM IS A FOLDER OF ITS OWN, named by --out and the arm together, so each is resumed,
    # combined and checked exactly as a run of one arm would be.

    def folder_of(arm):
        label = "-".join(part for part in (args.out, arm) if part)
        if label:
            return common.folder_for(label, TOOL, VERB, OUT_OPTION, kind)
        return common.run_folder(TOOL, VERB, OUT_OPTION, kind)

    outs = [folder_of(name) for name, _ in arms]

    # WHAT WAS NAMED WINS, INCLUDING UPWARDS. `default_workers` answers what the machine affords
    # and is asked only when nothing was named; somebody who knows what their box can take is not
    # second-guessed, and a run told `--workers 1` gets one whatever the cores say.
    workers = args.workers if args.workers is not None else default_workers()
    settle = Settling.of(args)
    # SPLIT THE WAY A SHELL WOULD, so `--driver="--save a name with spaces"` means what it looks
    # like it means whichever shell, or none, was between the person and this process. Every
    # arm takes it, ahead of its own arguments.
    shared = shlex.split(args.driver)
    drivers = [shared + own for _, own in arms]
    for out, driver in zip(outs, drivers):
        record_run(out, workers, settle, args.runs, named, kind, args.marking, driver)

    # BUILT ONCE FOR THE WHOLE PASS, BEFORE THE FIRST RUN, so every run of it is the same
    # binary BY CONSTRUCTION. Building per run made that a matter of nobody having touched the
    # tree meanwhile: an edit between run 2 and run 3 turned the next staleness check into a
    # real compile, and runs 1-2 and run 3 then measured different code under one revision in
    # run.json. Half a second is not why this is here - a pass that silently measures two
    # binaries is, and it looks exactly like a pass that measured one.
    #
    # A RESUME IS A NEW PASS and builds again, which is right: it is a separate invocation and
    # may be at a separate revision, which `write_run_record` already says out loud.
    #
    # AND ITS DIGEST IS TAKEN HERE and checked before every run, because building once only
    # binds what THIS process does. A cargo build started by hand in another window while a
    # pass is running relinks the file underneath it; see `Run.__init__`.
    #
    # ONE BUILD FOR EVERY ARM, for the same reason: arms compared against each other have to be
    # the same binary, so each arm's folder records the one build rather than one of its own.
    menus, did = build_measurement(MENUS, folder=outs[0])
    digest = did["sha256"]
    for out in outs:
        common.add_build_record(out, did)
    # THE ENUMERATION IS ITS OWN COMMAND, so it is its own build. Not part of the pass's
    # digest check: it decides WHICH groups are measured, and the check is about the binary
    # whose milliseconds the rows carry.
    groups, _ = build_measurement(GROUPS, folder=outs[0], quiet=True)
    try:
        # ONE ARM AFTER ANOTHER, AND ONE RUN AFTER ANOTHER, never side by side: two at once
        # would each be measuring how busy the other made the machine. Each keeps its own folder,
        # so a resume picks up what was interrupted and leaves the finished ones alone.
        for number, ((name, _), out, driver) in enumerate(zip(arms, outs, drivers), 1):
            prefix = args.status_prefix
            if len(arms) > 1:
                prefix += f"[Arm {number}/{len(arms)}]"
                print(f"\n##### arm {number} of {len(arms)}: {name} -> {out} #####")
            measure_arm(out, named, workers, settle, menus, digest, groups, args, driver, prefix)
        return 0
    except KeyboardInterrupt:
        print("\ninterrupted; what finished is on disk and a re-run resumes it")
        return 130


if __name__ == "__main__":
    sys.exit(main())
