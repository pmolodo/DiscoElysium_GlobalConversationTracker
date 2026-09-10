#!/usr/bin/env python

"""Measure a whole MENU per group, one group per process, several at a time.

A row of the option matrix is one search from one start. A request is a whole response menu
answered against one manager, so the menu is what a player waits for and it is not the sum of
its options. See measurements/menu_matrix.rs for what the columns mean.

Usage:
    tools/measure-menus.py [conversation ...|all]

Examples:
    tools/measure-menus.py 368 631      # just these two
    tools/measure-menus.py all          # every group in the game, resumably
    DEGCT_WORKERS=1 tools/measure-menus.py all   # one at a time, for timings worth trusting

ONE PROCESS PER GROUP for the reason the other drivers give: a group can take its process
down - conversation 28's deepest entries overflow the stack inside a recursive diagram
operation - and with every group in one process the first crash destroys every group after
it. A crash here is a RESULT for that group, recorded as CRASHED, and costs nothing else.

`all` ASKS THE OPTION MATRIX WHICH GROUPS EXIST, DEGCT_GROUPS_ONLY=1, rather than keeping a
list here. There is one enumeration in this repository and both drivers read it, so the two
cannot come to disagree about what the game contains. 901 of the game's 1,422 groups reach
nothing from their start and are recorded as NO-ROWS from the enumeration rather than by 901
processes that each build a graph to find the same nothing.

WORKERS AND TIMINGS PULL OPPOSITE WAYS, and this is the one thing to decide before running.
Groups in parallel finish the run several times sooner and make every millisecond column a
measurement of how busy the machine was. A BASELINE WANTS DEGCT_WORKERS=1. The default is
several because most runs of this are looking for a group that behaves oddly rather than for
a number to compare later, and the header of the output says which it was.

RESUMING. Rows are written as they finish, and pointing a later run at the same folder makes
it skip the groups already there:

    DEGCT_MENUS_OUT=measurements/logs/2026-09-09_menus tools/measure-menus.py all

The same command is the start and the resume; there is no separate mode to remember. Without
MENUS_OUT each run gets its own folder and resumes nothing.

WHAT COUNTS AS DONE:

    a row       measured, whatever it says. Done.
    CRASHED     the group took its process down. That IS the answer for that group.
    NO-MENU     no start of the group has anything worth hunting beyond it. Done.
    NOT-MEASURED  the machine could not supply the budget. NOT done - it is re-run.
"""

import argparse
import sys

from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import measurement_common as common  # noqa: E402  (after the path is set)

from measurement_common import (  # noqa: E402
    TAB,
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

# The example this drives, and the one it asks for the group list.
MENUS = "menu_matrix"
MATRIX = "performance_matrix"

# A verdict that means the row was never taken, so a resume takes it again.
RETRY = "NOT-MEASURED"


class Run:
    """One folder of rows, and what has already been written into it."""

    def __init__(self, out):
        self.folder = Path(out)
        self.folder.mkdir(parents=True, exist_ok=True)
        self.rows = self.folder / "menus.tsv"
        self.log = self.folder / "menus.log"
        self.menus = build_measurement(MENUS)
        self.matrix = build_measurement(MATRIX, quiet=True)

    def done(self):
        """The conversations already measured, so a resume can skip them.

        A ROW THAT SAYS NOT-MEASURED IS NOT DONE, because the machine could not supply the
        budget and nothing about the menu was learned. Every other row is an answer, a
        crash included.
        """
        if not self.rows.exists():
            return set()
        finished = set()
        for line in self.rows.read_text(encoding="utf-8", errors="replace").splitlines()[1:]:
            cells = line.split(TAB)
            if len(cells) < 5 or not cells[0].isdigit():
                continue
            if RETRY in cells:
                continue
            finished.add(int(cells[0]))
        return finished

    def header(self):
        """Writes the column names, asked of the measurement rather than written here."""
        if self.rows.exists():
            return
        answer = common.ask(self.menus, {qualified("HEADER"): "1"})
        common.write_lf(self.rows, answer.stdout)

    def groups(self):
        """Every group with something to measure, from the option matrix's enumeration."""
        answer = common.ask(self.matrix, {qualified("GROUPS_ONLY"): "1"})
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
            return f"{conversation}\tCRASHED\n", answer.stderr
        return answer.stdout, answer.stderr

    def append(self, rows):
        with common.open_lf(self.rows) as handle:
            handle.write(rows)


def measure(out, conversations, workers):
    run = Run(out)
    run.header()

    if conversations == ["all"]:
        conversations, empty = run.groups()
        print(f"{len(conversations)} group(s) with rows; {empty} reach nothing and are skipped")

    already = run.done()
    todo = [c for c in conversations if c not in already]
    if already:
        print(f"{len(already)} already measured in {run.folder}; {len(todo)} to go")

    if not todo:
        print("nothing to do.")
        return 0

    print(f"{len(todo)} group(s), {workers} at a time -> {run.rows}")

    state = {"done": 0}

    def work(conversation):
        return run.measure(conversation)

    def reap(conversation, result):
        rows, errors = result
        if rows.strip():
            run.append(rows)
        if errors.strip():
            with common.open_lf(run.log) as handle:
                handle.write(f"=== {conversation} ===\n{errors}")
        state["done"] += 1
        print(progress_line(state["done"], len(todo), f"conversation {conversation}"))

    run_groups(todo, work, workers, reap)

    print(f"\n{state['done']} group(s) measured -> {run.rows}")
    if run.log.exists():
        print(f"what the groups said on stderr is in {run.log}")
    return 0


###############################################################################
# CLI
###############################################################################


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
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)

    named = args.conversations or ["all"]
    if named != ["all"]:
        named = [int(c) for c in named]

    # NAMED, OR A FOLDER OF ITS OWN. A run told where to write resumes what is there; one
    # that is not gets a fresh folder and resumes nothing, which is the safe default - a
    # resume into a folder taken against different settings would mix two measurements.
    out = env("MENUS_OUT") or common.run_folder("menus", "MENUS_OUT")

    workers = env_int("WORKERS", default_workers())
    try:
        return measure(out, named, workers)
    except KeyboardInterrupt:
        print("\ninterrupted; what finished is on disk and a re-run resumes it")
        return 130


if __name__ == "__main__":
    sys.exit(main())
