#!/usr/bin/env python

"""Measure a whole MENU per group, one group per process: the heavy groups one at a time, then
the rest several at a time.

A request is a whole response menu answered against one manager, so the menu is what a player
waits for and it is not the sum of its options. See measurements/menu_matrix.rs for what the
columns mean.

Usage:
    tools/measure-menus.py [conversation ...|all]

Examples:
    tools/measure-menus.py 368 631      # just these two
    tools/measure-menus.py all          # every group in the game, resumably
    DEGCT_WORKERS=1 tools/measure-menus.py all   # every group one at a time

ONE PROCESS PER GROUP for the reason the other drivers give: a group can take its process
down - conversation 28's deepest entries overflow the stack inside a recursive diagram
operation - and with every group in one process the first crash destroys every group after
it. A crash here is a RESULT for that group, recorded as CRASHED, and costs nothing else.

`all` ASKS THE MENU MATRIX WHICH GROUPS EXIST, DEGCT_GROUPS_ONLY=1, rather than keeping a
list here, so what is in a run is decided by the index and nothing else. 901 of the game's
1,422 groups reach nothing from their start and are skipped from the enumeration rather than
by 901 processes that each build a graph to find the same nothing. The enumeration arrives
heaviest-first, which is what the serial phase below rests on.

THE HEAVY GROUPS ARE MEASURED ONE AT A TIME, because workers and timings pull opposite ways:
groups in parallel finish the run several times sooner and make every millisecond column a
measurement of how busy the machine was - and the heavy groups are the ones whose milliseconds
anybody reads. So the run measures groups one at a time, heaviest first, until the cost has
bottomed out, and only then hands the rest to DEGCT_WORKERS at once. The rule lives in
`measurement_common.Settling`: DEGCT_SETTLE_GROUPS settled groups in a
row, a group counting as settled when it is within DEGCT_SETTLE_FACTOR of the cheapest menu so
far or under DEGCT_SETTLE_MS outright. At least DEGCT_SETTLE_GROUPS groups are always measured
one at a time.

WHAT COUNTS TOWARDS SETTLING. A measured menu counts by its `menu_ms`. A CRASHED or
NOT-MEASURED group resets the count, since it is evidence that something did not measure
rather than that measuring got cheap. A NO-MENU group neither counts nor resets: no start of it
had anything worth hunting, which says nothing about what the next menu costs.

RESUMING. Rows are written as they finish, and pointing a later run at the same folder makes
it skip the groups already there:

    DEGCT_MENUS_OUT=measurements/logs/2026-09-09_menus tools/measure-menus.py all

The same command is the start and the resume; there is no separate mode to remember. A resumed
group still counts towards settling, by the row it left, so a resume switches where the
original run would have. Without MENUS_OUT each run gets its own folder and resumes nothing.

SEVERAL RUNS. `--runs N` takes N runs of the same groups back to back, each in run-1 ... run-N
under the one folder, because a single run's milliseconds are a reading of the machine as much
as of the search. When N is more than one it then writes combined.tsv - each group's median,
min and max menu_ms, its median nodes, and its rounds, settled and starred with a flag for
whether every run agreed - and summary.txt, the per-run totals and the costliest groups, and
prints the summary:

    DEGCT_WORKERS=1 tools/measure-menus.py --runs 3 all

WHAT COUNTS AS DONE:

    a row       measured, whatever it says. Done.
    CRASHED     the group took its process down. That IS the answer for that group.
    NO-MENU     no start of the group has anything worth hunting beyond it. Done.
    NOT-MEASURED  the machine could not supply the budget. NOT done - it is re-run.
"""

import argparse
import statistics
import sys

from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import measurement_common as common  # noqa: E402  (after the path is set)

from measurement_common import (  # noqa: E402
    COMBINED,
    SUMMARY,
    TAB,
    Settling,
    build_measurement,
    default_workers,
    env,
    env_int,
    progress_line,
    qualified,
    refuse,
    run_groups,
)

###############################################################################
# Core functions
###############################################################################

# The example this drives, which is also what it asks for the group list.
MENUS = "menu_matrix"

# A verdict that means the row was never taken, so a resume takes it again.
RETRY = "NOT-MEASURED"

# The verdict for a group whose process died, written by this driver.
CRASHED = "CRASHED"

# The verdict for a group with no menu worth asking about.
NO_MENU = "NO-MENU"

# The column a menu's cost is in, and where a verdict goes instead for an unmeasured group.
MENU_MS = "menu_ms"

# The absolute arm of the settle rule for a menu. Menus bottom out near twenty milliseconds - a
# matrix group, many rows long, near two seconds - so a menu under this is at the floor whatever
# the relative arm says of it.
SETTLE_MS = 100


class Run:
    """One folder of rows, and what has already been written into it."""

    def __init__(self, out):
        self.folder = Path(out)
        self.folder.mkdir(parents=True, exist_ok=True)
        self.rows = self.folder / "menus.tsv"
        self.log = self.folder / "menus.log"
        self.menus = build_measurement(MENUS)

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
        answer = common.ask(self.menus, {qualified("HEADER"): "1"})
        common.write_lf(self.rows, answer.stdout)

    def columns(self):
        """The column names, off the header this run's rows were written under."""
        return self.rows.read_text(encoding="utf-8", errors="replace").splitlines()[0].split(TAB)

    def groups(self):
        """Every group with something to measure, heaviest first, from the menu matrix."""
        answer = common.ask(self.menus, {qualified("GROUPS_ONLY"): "1"})
        if answer.returncode != 0:
            refuse(f"the group enumeration failed:\n{answer.stderr}", code=1)

        common.write_lf(self.folder / "groups.tsv", answer.stdout)

        wanted, empty = [], 0
        for line in answer.stdout.splitlines():
            cells = line.split(TAB)
            if len(cells) < 4 or not cells[0].lstrip("-").isdigit():
                continue
            start, reachable = int(cells[0]), int(cells[3])
            if reachable > 0:
                wanted.append(start)
            else:
                empty += 1
        return wanted, empty

    def measure(self, conversation):
        """One group, in a process of its own."""
        answer = common.ask(
            self.menus,
            common.env_for_child(CONVERSATION=str(conversation)),
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
    """What one group's row says about its cost: (ms, complete), or None for a NO-MENU group.

    See the module doc for why a crash resets the settle count and a group with no menu does
    not.
    """
    if CRASHED in cells:
        return 0, False
    at = columns.index(MENU_MS)
    if at >= len(cells):
        return 0, False
    cell = cells[at]
    if cell == NO_MENU:
        return None
    if not cell.isdigit():
        return 0, False
    return int(cell), True


def serial_phase(run, conversations, recorded, workers, settle, reap):
    """The heavy groups one at a time, until their cost has bottomed out.

    Returns how many of `conversations`, in order, the phase covered. A group already recorded
    is not measured again, but its row still counts towards settling.
    """
    columns = run.columns()
    for position, conversation in enumerate(conversations, start=1):
        if conversation in recorded:
            cells = recorded[conversation]
        else:
            rows, errors = run.measure(conversation)
            reap(conversation, (rows, errors))
            first = rows.splitlines()[0] if rows.strip() else f"{conversation}\t{CRASHED}"
            cells = first.split(TAB)

        if workers <= 1:
            continue

        cost = menu_cost(cells, columns)
        if cost is None:
            continue
        menu_ms, complete = cost
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


def measure(out, conversations, workers):
    run = Run(out)
    run.header()

    if conversations == ["all"]:
        conversations, empty = run.groups()
        print(f"{len(conversations)} group(s) with rows; {empty} reach nothing and are skipped")

    recorded = run.recorded()
    todo = [c for c in conversations if c not in recorded]
    if recorded:
        print(f"{len(recorded)} already measured in {run.folder}; {len(todo)} to go")

    if not todo:
        print("nothing to do.")
        return 0

    settle = Settling.from_env(SETTLE_MS)
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
        if errors.strip():
            with common.open_lf(run.log) as handle:
                handle.write(f"=== {conversation} ===\n{errors}")
        state["done"] += 1
        print(progress_line(state["done"], len(todo), f"conversation {conversation}"))

    serial_done = serial_phase(run, conversations, recorded, workers, settle, reap)

    remaining = [c for c in conversations[serial_done:] if c not in recorded]
    if remaining:
        run_groups(remaining, run.measure, workers, reap)

    print(f"\n{state['done']} group(s) measured -> {run.rows}")
    if run.log.exists():
        print(f"what the groups said on stderr is in {run.log}")
    return 0


# Where each of several runs is written, under the folder the runs share.
RUN_FOLDER = "run-{}"

# How many of the costliest groups the summary lists.
HARDEST = 10

# What a run's outcome is judged steady on: columns that say what the marking decided rather
# than what it cost, so they should not move between runs at all.
OUTCOME = ("rounds", "settled", "starred")

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


def combine(folders, out):
    """Folds several runs of the same groups into one table and a summary, and prints it.

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


def record_run(out, workers, runs, named):
    """Writes what this run is - see `measurement_common.write_run_record` - into its folder.

    PARALLELISM IS THE PART THAT CHANGES WHAT THE ROWS SAY: groups measured side by side pay a flat
    cost in setup that groups measured alone do not - about ten milliseconds a small group,
    measured 2026-09-14 - so `tools/menu-costs-diff.py` refuses to compare folders that disagree
    about it, and a resume into a folder recorded at other parallelism is refused.
    """
    parallelism = {
        "workers": workers,
        "settle": Settling.from_env(SETTLE_MS).rule() if workers > 1 else None,
    }
    common.write_run_record(out, parallelism, runs=runs, groups=named)


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
        help="how many runs to take back to back; more than one writes each under run-N and combines them",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    if args.runs < 1:
        refuse(f"--runs {args.runs}: a run count is at least 1")

    named = args.conversations or ["all"]
    if named != ["all"]:
        named = [int(c) for c in named]

    # NAMED, OR A FOLDER OF ITS OWN. A run told where to write resumes what is there; one
    # that is not gets a fresh folder and resumes nothing, which is the safe default - a
    # resume into a folder taken against different settings would mix two measurements.
    out = Path(env("MENUS_OUT") or common.run_folder("menus", "MENUS_OUT"))

    workers = env_int("WORKERS", default_workers())
    record_run(out, workers, args.runs, named)
    try:
        if args.runs == 1:
            return measure(out, named, workers)
        # ONE RUN AFTER ANOTHER, never side by side: two runs at once would each be measuring
        # how busy the other made the machine. Each keeps its own folder, so a resume picks up
        # the run that was interrupted and leaves the finished ones alone.
        folders = []
        for number in range(1, args.runs + 1):
            folder = out / RUN_FOLDER.format(number)
            print(f"\n=== run {number} of {args.runs} -> {folder} ===")
            measure(folder, named, workers)
            folders.append(folder)
        combine(folders, out)
        return 0
    except KeyboardInterrupt:
        print("\ninterrupted; what finished is on disk and a re-run resumes it")
        return 130


if __name__ == "__main__":
    sys.exit(main())
