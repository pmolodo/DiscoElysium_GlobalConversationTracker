#!/usr/bin/env python

"""Take the unreachable census ONE GROUP PER PROCESS, several at a time.

The census asks, per group, which of its entries no path can reach under the world's
conditions - deepest first, up to ten. See the `census` function in
measurements/performance_matrix.rs for what the columns mean and what `at-least` and
`undecided` are hiding when they are not zero.

Usage:
    tools/measure-census.py [conversation ...|all]

Examples:
    tools/measure-census.py 368 631      # just these two
    tools/measure-census.py all          # every group in the game, resumably
    DEGCT_WORKERS=1 tools/measure-census.py all   # one at a time, the way it used to run

ONE PROCESS PER GROUP for the reason the matrix driver gives at length: a group can take its
process down - conversation 28's deepest entries overflow the stack inside a recursive diagram
operation - and with every group in one process the first crash destroys every group after it.
A crash here is a RESULT for that group, recorded as CRASHED, and costs nothing else.

`all` asks the measurement which groups exist and which have anything in them, exactly as the
matrix does, rather than reading a list kept here that could omit a group and never say so.
901 of the game's 1,422 groups reach nothing from their start and are recorded as NO-ROWS from
the enumeration - a third of a second for the whole game - instead of by 901 processes that
each build a graph to find the same nothing.

RESUMING. Rows are written as they finish, and pointing a later run at the same folder makes
it skip the groups already there:

    DEGCT_CENSUS_OUT=measurements/logs/2026-09-08_census tools/measure-census.py all

The same command is the start and the resume; there is no separate mode to remember. Without
CENSUS_OUT each run gets its own folder and resumes nothing.

AND IT RESUMES INSIDE A GROUP TOO. Each group writes `groups/<conv>.journal.tsv`, one line per
candidate as its verdict is established, and a re-run of that group skips what is already
there instead of asking again. Without it an interrupted group cost everything spent on it,
which for a group full of candidates the pass cannot settle - five seconds each - is hours.
See `Journal` in measurements/symbolic_answers.rs.

THE JOURNALS ARE ALSO THE PER-ENTRY ANSWER, and worth keeping for that alone: a census row
names only what it PROVED unreachable, while the journal names every candidate and what was
established about it, reachable ones included.

WHAT COUNTS AS DONE:

    a row       measured, whatever it says. Done.
    CRASHED     the group took its process down. That IS the answer for that group.
    NO-ROWS     the enumeration says there is nothing to census here. Also an answer.

STOPPING IT MID-RUN NEEDS MORE THAN KILLING THE SHELL, exactly as it does for the matrix
driver and for the same reason: one process per group means killing the terminal or the job
leaves this loop spawning new ones, which then hold the measurement binary open and fail the
next build with LNK1104, from a run nobody thinks is still going.

    tools/stop-measurements.sh --list     # what is running
    tools/stop-measurements.sh            # stop it

Nothing is lost but the groups in flight: rows are written as they finish, so the same command
with the same CENSUS_OUT picks up where it stopped.

## What de-42lg changed, and what it deliberately did not

PARALLEL, because the census was the serial third of a whole-game run: 521 groups one at a
time, about eight minutes, and paid twice on a fresh whole-game matrix run - once for the
census the grid's unreachable profiles need, then again by the row phase, which was already
parallel.

A GROUP OWNS ITS OWN ROW FILE, `groups/<conv>.row.tsv`, and census.tsv is ASSEMBLED from
those. Several workers appending to one file is the thing to get right rather than hope about
- a short line is often atomic and "often" is not a property to build on, especially through
msys on Windows. It also removes a cost that was there before any of this: the old resume
scanned the whole of a growing census.tsv once per group, 521 times over a whole-game run,
where a file test does.

census.tsv STAYS THE ARTEFACT, because the matrix driver reads it by name and so does
everything downstream. Only how it comes to exist changes, and the row ORDER is the same it
always was - the enumeration's order, heaviest group first - because that is the order the
assembly walks.

THE BUDGET IS NOT DIVIDED HERE, which is the one place this differs from the matrix driver on
purpose. A matrix row is allowed the full measurement budget - six gigabytes - so four of them
at once would commit twenty-four, and the matrix divides one allowance among its workers. A
census process is allowed `DiagramBudget::over_a_group()`, a fixed 512 MB that nothing on the
command line moves, so four workers really do commit four times as much and there is nothing
to divide.

THAT IS WHY THE WORKER COUNT IS BOUNDED BY MEMORY AS WELL AS BY CORES here and not there. It
is the smaller of the logical processors and what the free memory affords at 512 MB apiece,
holding back five per cent of total memory for the machine to keep working in - see
`measurement_common.default_workers`. DEGCT_WORKERS=n overrides it either way.

THE JOURNALS MAKE AN OVER-AMBITIOUS CHOICE CHEAP TO RECOVER FROM. A group killed for want of
memory resumes inside itself rather than from the start.

## The `ms` column of a parallel census is not comparable with a serial one

Verified 2026-09-09 on the whole game, both ways, 521 groups: EVERY COLUMN BUT `ms` is
identical, which is the acceptance test de-42lg asked for - a census is a fact about the
content, not about the machine, so anything else that moved would be a bug rather than a
speed-up.

`ms` moves, and by a lot. Summed over the 521 groups it went from 17.9 seconds serial to 41.2
parallel on four workers, because each group's own clock now includes contention with three
others. The WALL clock is what improved, 3:43 to 2:48. So a group that reads slower in a
parallel census has not got slower; read the wall clock for the run and the other columns for
the answer.
"""

import argparse
import os
import subprocess
import sys
import time
import traceback

from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from measurement_common import (  # noqa: E402
    ROOT,
    TAB,
    build_measurement,
    default_workers,
    env,
    env_for_child,
    open_lf,
    progress_line,
    qualified,
    refuse,
    run_groups,
    write_lf,
)

# What a census process is allowed, mirrored here only to be REPORTED. It is
# DiagramBudget::over_a_group() and nothing on the command line moves it, which is why the
# worker count is not divided; see the module docstring.
CENSUS_MEMORY_MB = 512


class Census:
    def __init__(self, out):
        self.out = out
        self.groups_dir = out / "groups"
        self.groups_dir.mkdir(parents=True, exist_ok=True)
        self.rows = out / "census.tsv"
        self.binary = build_measurement()
        self.started = time.monotonic()
        self.done = 0
        self.measured = 0
        self.crashed = 0
        self.total = 0

    def row_file(self, conversation):
        return self.groups_dir / f"{conversation}.row.tsv"

    def adopt_existing_rows(self):
        """Split a census.tsv this folder already holds into per-group row files.

        WHY A FOLDER CAN HOLD ONE WITHOUT THE OTHER: every census taken before de-42lg
        appended straight to census.tsv, and those folders are not historical - the matrix
        driver reuses the newest of them on every run, and resuming into one is the normal way
        a long census is finished. Reading only the per-group files would re-census a folder
        that is already complete, which is eight minutes on the whole game.

        THE LAST ROW PER CONVERSATION WINS, which is the rule the whole census reads by: the
        file was appended to, so a retried group sits after the one it replaces.
        """
        if not self.rows.exists():
            return 0
        adopted = 0
        for line in self.rows.read_text(encoding="utf-8", errors="replace").splitlines():
            cells = line.split(TAB)
            if len(cells) < 2 or cells[0] in ("conv", ""):
                continue
            target = self.row_file(cells[0])
            if target.exists():
                continue
            write_lf(target, line + "\n")
            adopted += 1
        return adopted

    def header(self):
        """The header, from the measurement rather than from a copy kept here."""
        answer = subprocess.run(
            [str(self.binary)],
            capture_output=True,
            text=True,
            errors="replace",
            env=env_for_child(CENSUS="1", CONVERSATION="-1"),
        )
        for line in answer.stdout.splitlines():
            if line.startswith("conv"):
                return line
        refuse("could not read the census columns from the measurement", code=1)

    def enumerate_groups(self):
        """Which groups exist, and which have anything to census. One pass for both.

        Building each group's graph is what settles the second question, and doing it twice
        would be the expensive half done twice.
        """
        print("enumerating the game's groups...")
        answer = subprocess.run(
            [str(self.binary)],
            capture_output=True,
            text=True,
            errors="replace",
            env=env_for_child(GROUPS_ONLY="1"),
        )
        write_lf(self.out / "groups.tsv", answer.stdout)
        write_lf(self.out / "groups.log", answer.stderr)

        found = []
        empty = []
        for line in answer.stdout.splitlines():
            cells = line.split(TAB)
            if len(cells) < 4 or not cells[0].strip():
                continue
            if int(cells[3]) > 0:
                found.append(int(cells[0]))
            else:
                empty.append(cells[0])

        write_lf(self.out / "skipped.tsv", "".join(f"{start}{TAB}NO-ROWS\n" for start in empty))
        print(f"  {len(found)} group(s) to census, {len(empty)} recorded NO-ROWS")
        return found

    def census_one(self, conversation):
        """One group, in its own process, into its own row file.

        Returns (verdict, seconds) for the reap line. The row is on disk before this returns,
        so a kill loses the group in flight and nothing else.
        """
        log = self.groups_dir / f"{conversation}.log"
        began = time.monotonic()
        with open_lf(log, "w") as handle:
            status = subprocess.run(
                [str(self.binary)],
                stdout=handle,
                stderr=subprocess.STDOUT,
                env={
                    **dict(os.environ),
                    qualified("CENSUS"): "1",
                    qualified("NO_HEADER"): "1",
                    qualified("CONVERSATION"): str(conversation),
                    qualified("CENSUS_JOURNAL"): str(self.groups_dir / f"{conversation}.journal.tsv"),
                },
            ).returncode
        took = time.monotonic() - began

        text = log.read_text(encoding="utf-8", errors="replace")
        rows = [line for line in text.splitlines() if line.startswith(f"{conversation}{TAB}")]
        if status != 0 or not rows:
            # A CRASH IS THE ANSWER FOR THIS GROUP, written down so a resume does not retry it
            # for ever and a reader can tell it from a group nobody ran.
            write_lf(
                self.row_file(conversation), f"{conversation}{TAB}CRASHED{TAB}{TAB}{TAB}{TAB}{int(took * 1000)}{TAB}\n"
            )
            return f"CRASHED exit {status}, see {log}", took

        write_lf(self.row_file(conversation), rows[-1] + "\n")
        # NOTHING TO SAY ABOUT A GROUP THAT WORKED. The duration is printed by the caller, in
        # its own fixed-width column, so that 521 of these lines can be read down rather than
        # across. de-12wr.7.
        return "", took

    def assemble(self, order):
        """census.tsv, from the per-group row files, in the enumeration's order.

        WRITTEN WHOLE RATHER THAN APPENDED TO, which is what makes several workers safe: no
        two of them ever touch this file, and it is rebuilt from what is on disk whenever the
        run has something new to put in it. The order is the one the serial version produced,
        so a folder assembled here is comparable with one appended to before de-42lg.
        """
        lines = [self.header()]
        for conversation in order:
            path = self.row_file(conversation)
            if not path.exists():
                continue
            lines.extend(line for line in path.read_text(encoding="utf-8").splitlines() if line.strip())
        write_lf(self.rows, "\n".join(lines) + "\n")


def measure(named):
    out = Path(env("CENSUS_OUT") or (ROOT / "measurements" / "logs" / f"{time.strftime('%Y-%m-%d_%H,%M,%S')}_census"))
    if not out.is_absolute():
        out = ROOT / out

    census = Census(out)

    if not named or (len(named) == 1 and named[0] == "all"):
        groups = census.enumerate_groups()
    else:
        groups = []
        for value in named:
            try:
                groups.append(int(value))
            except ValueError:
                refuse(f"{value!r} is not a conversation id")
        write_lf(out / "skipped.tsv", "")

    adopted = census.adopt_existing_rows()
    if adopted:
        print(f"  {adopted} group(s) already in this folder's census.tsv, adopted as done")

    todo = [g for g in groups if not census.row_file(g).exists()]
    census.total = len(groups)
    census.done = len(groups) - len(todo)

    # WHAT ONE CENSUS PROCESS COMMITS, handed over so the worker count is bounded by the
    # memory as well as by the cores. Not divided, because nothing divides it: a census
    # process takes DiagramBudget::over_a_group() whatever else is running.
    workers = default_workers(CENSUS_MEMORY_MB)
    print(
        f"{len(todo)} group(s) to run of {len(groups)}, {workers} at a time, "
        f"{CENSUS_MEMORY_MB} MB each, started {time.strftime('%H:%M:%S')}"
    )
    if census.done:
        print(f"resuming in {out}: {census.done} group(s) already done")

    # THE WIDEST CONVERSATION ID THE RUN WILL MEET, so the item column is as fixed as the
    # count is. The rest of the layout is `progress_line`'s, shared with the matrix driver.
    id_width = max((len(str(g)) for g in groups), default=4)

    def reap(conversation, result):
        note, seconds = result
        census.done += 1
        census.measured += 1
        if note.startswith("CRASHED"):
            census.crashed += 1
        elapsed = time.monotonic() - census.started
        left = len(todo) - census.measured
        # PACED ON WHAT THIS RUN ACTUALLY MEASURED, and divided by the concurrency it is
        # actually achieving rather than by the worker count - the same reasoning the matrix
        # driver's parallel estimate carries. Groups differ by orders of magnitude in cost, so
        # slots sit empty and the run drains at the end with fewer groups left than workers.
        estimate = elapsed * left / census.measured if census.measured else 0
        # THE PERCENTAGE IS OF THE WHOLE RUN, not of what is left to do, so a resume that
        # skipped four hundred groups opens near the number it deserves rather than at zero.
        # de-12wr.5, and `progress_line` is where it lives now.
        print(
            progress_line(
                census.done,
                census.total,
                f"{conversation:>{id_width}}",
                seconds=seconds,
                elapsed=elapsed,
                estimate=estimate,
                note=note,
            ),
            flush=True,
        )
        # ASSEMBLED AS IT GOES, not only at the end, so a run that is killed still leaves a
        # readable census.tsv covering everything it had finished. Rebuilding it per group is
        # a few hundred short files read; the alternative is a folder whose artefact only
        # exists if the run was allowed to finish.
        census.assemble(groups)

    run_groups(todo, census.census_one, workers, reap)
    census.assemble(groups)

    print()
    print(f"census: {census.rows}")
    if census.crashed:
        print(f"{census.crashed} group(s) CRASHED; their rows say so and a resume keeps them.")


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "conversations",
        nargs="*",
        help="conversation ids to census, or `all` for every group in the game",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        measure(args.conversations)
    except SystemExit:
        raise
    except KeyboardInterrupt:
        print("\nstopped; re-run the same command with the same CENSUS_OUT to resume")
        return 130
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
