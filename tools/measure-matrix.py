#!/usr/bin/env python

"""Run the performance matrix ONE ROW PER PROCESS, keeping every row that survives.

A row can take the whole process down. Measured: conversation 28 with its five deepest
entries unseen overflows the stack inside a recursive diagram operation, and the run that
found that reported two rows out of eleven and lost the other nine. Others die by the
diagram manager running out of nodes, or by a single step running minutes past its budget.

One process per row means a crash is a RESULT for that row - recorded as such - and costs
nothing else. That is the same bargain tools/measure-symbolic.sh makes, for the same reason;
this one is finer grained because the matrix has far more rows per conversation.

Usage:
    tools/measure-matrix.py [conversation ...|all]

Examples:
    tools/measure-matrix.py                 # the six heavy conversations, every profile
    tools/measure-matrix.py 368 631         # just these two
    ENGINES=bwd tools/measure-matrix.py 14  # one engine, one group
    PROFILES=deepest-1 ENGINES=bwd tools/measure-matrix.py 14   # one row
    PROFILES=95pc-seen,50pc-seen tools/measure-matrix.py 14     # a held-back profile
    tools/measure-matrix.py all             # EVERY group in the game, resumably

THE DEFAULT GRID IS FIVE DEEP PROFILES - deepest-1, -5, -10, deepest-unreach-1 and -5. The
seven percentage-seen profiles that used to be in it were measured to be too easy to be
worth a run's time and are held back, nameable by PROFILES=. See PROFILES and TOO_EASY in
measurements/performance_matrix.rs for the table.

A GRID WITH AN UNREACHABLE PROFILE NEEDS A CENSUS, and this takes one into the run's own
folder if CENSUS_FILE names none and the folder holds none - see `arrange_census`, below,
for why it is taken there and why nothing tries to decide that an existing one is out of
date.

`all` asks the measurement itself which groups exist - GROUPS_ONLY=1, one canonical start
per distinct closure, heaviest first - rather than reading a list kept here, which could
omit a group and never say so. It is 1,422 groups against the six a default run does.

AND IT ASKS WHICH OF THEM HAVE ANYTHING IN THEM. 901 of the 1,422 reach nothing from their
start, mostly the two-entry ORB stubs the database is full of. They are SKIPPED ENTIRELY -
no file, no row - and the enumeration that identifies them costs a third of a second for the
whole game, against nine thousand processes that would each build a graph to find the same
nothing. Which groups those were, and why, is in groups.log; the whole enumeration is in
groups.tsv. See `enumerate_groups` for why that answer is asked for and not cached, and
de-cziy for why they stopped being written down.

NAMING ONE ON THE COMMAND LINE IS AN ERROR, since the only way the row loop can meet a group
with no rows is that a person typed it - `all` prunes them first.

RESUMING. A run writes its rows as it finishes them, and pointing a later run at the same
folder makes it skip what is already there:

    MATRIX_OUT=measurements/logs/2026-09-07_whole-game tools/measure-matrix.py all

Run that again after a kill, a crash, or a reboot and it picks up where it stopped. It is
the same command every time - there is no separate resume mode to remember, and no way to
resume into the wrong folder by forgetting a flag. Without MATRIX_OUT each run gets its own
folder, as before, and resumes nothing.

WHAT COUNTS AS DONE, because the three outcomes are not alike:

    ok            measured. Done.
    CRASHED       the row took its process down. That IS the answer for that row, recorded
                  as such, and a resume must not retry it for ever.
    (no rows)     the measurement said there is nothing to measure - no group builds from
                  this start, no entry 0, or nothing reachable. NOT A ROW ANY MORE: an
                  enumerated group like this is skipped silently and counted, and a NAMED one
                  stops the run. Folders written before de-cziy still hold NO-ROWS rows and
                  are read as they always were.
    NOT-MEASURED  the machine could not supply the memory budget. NOTHING WAS MEASURED, so
                  this is the one outcome a resume retries.

THE ROW IN FLIGHT IS LOST, and that is accepted rather than overlooked. Recovering it would
mean writing a marker before the row and reasoning about markers with no row after them; the
cost of not doing it is one row out of fourteen thousand, re-measured.

Rows are APPENDED as they finish, so a kill -9 keeps everything before it. A retried
NOT-MEASURED row therefore leaves both lines in the file, in the order they happened; READ
THE LAST ROW PER (conv, profile), which is what the resume itself does.

ENGINES and PROFILES each take a comma or space separated list and narrow the grid the same
way the conversation arguments do. The engines are fwd (the symbolic forward search), bwd
(the backward one), ingame and nolimit (the switching method the game runs, at the player's
own settings and with the limits off - de-xegj split what was one fwdbwd column, because one
column cannot say both what a player waits for and where the method actually stops). `all`
measures every one of them.

Older folders hold a single `fwdbwd` column instead; read the header, which is what
`RowWeights` and `group_cost` both do.

STOPPING IT MID-RUN NEEDS MORE THAN KILLING THE SHELL. Every row is a fresh measurement
process, so killing the terminal or the job leaves the loop looping and starting new ones -
which then hold target/release/examples/performance_matrix.exe open and fail the next build
with LNK1104, from a run nobody thinks is still going.

    tools/stop-measurements.sh --list     # what is running
    tools/stop-measurements.sh            # stop it

Writes one folder per run under measurements/logs, holding a log per row AND the TSV each
conversation's rows were collected into.

## Why this is Python and was shell

de-12wr.1. The driver is process orchestration with arithmetic over a table, and the shell
could do the first and not the second - so the second lived in tools/matrix-remaining.awk
and tools/matrix-group-cost.awk, and the seam between the three languages is where this
project's driver bugs lived. Both are folded in here, as `RowWeights` and `group_cost`.

THREE THINGS THE PORT ABSORBED rather than translated. The awk helpers, above. The
`conv:profile=seconds;...` strings the estimate passed around, which were a data structure
spelled as text because the shell had nowhere else to put one. And the whole-second clocks:
a row was timed by differencing `date +%s`, so a 1.4-second row recorded as 1 or 2, and
ROW_OVERHEAD - the largest single term in a row's weight - had to be a hand-set constant
because nothing could measure it. It is measured now; see `RowWeights.overhead`.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import threading
import time
import traceback

from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import measurement_common as common  # noqa: E402

###############################################################################
# Where things are
###############################################################################

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "measurements"

TAB = "\t"

# The six heavy conversations the matrix has always meant, when nothing is named.
DEFAULT_CONVERSATIONS = [362, 368, 631, 14, 28, 1030]

# THE DEEP PROFILES, AND ONLY THOSE. The seven percentage-seen ones were measured to be too
# easy to be worth a run's time - about a fifth of a second whatever the group, and one
# single answer between all seven on 92.5 per cent of them - and are held back. They are
# still runnable by name: PROFILES=95pc-seen,50pc-seen. See PROFILES and TOO_EASY in
# measurements/performance_matrix.rs for the table this came from.
DEFAULT_PROFILES = [
    "deepest-1",
    "deepest-5",
    "deepest-10",
    "deepest-unreach-1",
    "deepest-unreach-5",
]

# WHAT A ROW COSTS BEFORE ITS ENGINES START, in seconds, when this run has not yet measured
# it for itself. Every row is a fresh process that reads the index, builds the graph and lays
# out the variables, and none of that is in any `_ms` column - those time the engines only.
# See `RowWeights.overhead`, which replaces this with a measurement as soon as there is one.
FALLBACK_ROW_OVERHEAD = 1.4

# The measurement's own defaults, used only when the binary is too old to print its
# constants. See `read_constants`.
FALLBACK_MEMORY_MB = 6144
FALLBACK_BYTES_PER_NODE = 40
FALLBACK_ROW_SECONDS = 600


def refuse(message, code=2):
    """Stops the run, saying why, rather than measuring something nobody asked for."""
    print(message, file=sys.stderr)
    raise SystemExit(code)


def clock(seconds):
    """h:mm:ss. A run of this length is watched rather than read afterwards."""
    seconds = max(0, int(seconds))
    return f"{seconds // 3600}:{(seconds % 3600) // 60:02d}:{seconds % 60:02d}"


def env_list(name):
    """A comma or space separated environment list, or None where it is unset or empty."""
    raw = os.environ.get(name, "").strip()
    if not raw:
        return None
    return [piece for piece in re.split(r"[,\s]+", raw) if piece]


def env_int(name, fallback):
    raw = os.environ.get(name, "").strip()
    if not raw:
        return fallback
    try:
        return int(raw)
    except ValueError:
        refuse(f"{name}={raw!r} is not a number")


###############################################################################
# What a row has cost before, which is what the estimate is weighted by
###############################################################################


class RowWeights:
    """How long a run has left, weighted by what each row has cost before.

    The flat estimate this replaces - the mean row so far, spread over the rows that remain -
    assumes the rows are alike, and they are not. Each conversation's adversarial profiles
    run first and are the slowest, so a flat estimate reads long early and short late; and a
    run that narrows the grid to one engine would be scaled from rows measured with three.

    WHAT THIS DOES INSTEAD. Past runs already record what every (engine, conversation,
    profile) cost, in the per-engine ms columns of every performance-matrix-*.tsv. Average
    those into a WEIGHT per row, then let this run's completed rows say how fast this machine
    is running today:

        ratio    = seconds actually spent on the rows already done
                   / the summed weight of those same rows
        estimate = ratio * the summed weight of the rows not yet run

    So three rows that have cost 5, 3 and 2 minutes before, whose first row has just taken 10
    rather than 5, have 10 minutes left rather than 5: the pace has changed and the SHAPE of
    what remains has not.

    `estimate` returns None when it cannot answer - no history, no completed row to calibrate
    against, or nothing left to estimate. The caller falls back to the flat mean and says
    that it has.
    """

    def __init__(self, engines, tsvs, row_overhead=None):
        self.engines = list(engines)
        # Sums and counts, at four levels of specificity. FOUR FALLBACKS, because a tuple
        # with no history must not silently weigh nothing - a row dropped from the total is a
        # row the estimate says is free. A conversation is a better proxy for an unmeasured
        # profile than a profile is for an unmeasured conversation, so (engine, conversation)
        # is tried before (engine, profile).
        self.total = {}
        self.count = {}

        # ROW_OVERHEAD=n still moves it and 0 still restores weighting by engine time alone;
        # what changed is that leaving it unset now means "measure it" rather than "use 1.4".
        self.fixed_overhead = row_overhead
        self.overhead_total = 0.0
        self.overhead_count = 0

        for tsv in tsvs:
            self._read(tsv)

    def _add(self, key, seconds):
        self.total[key] = self.total.get(key, 0.0) + seconds
        self.count[key] = self.count.get(key, 0) + 1

    def _read(self, path):
        try:
            lines = Path(path).read_text(encoding="utf-8", errors="replace").splitlines()
        except OSError:
            return
        if not lines:
            return

        header = lines[0].split(TAB)

        # WHICH ERA THIS FILE IS FROM, decided before anything is renamed, because `fwd` has
        # meant two different searches and only the company it keeps says which.
        #
        #   oldest      fwd, bwd                 - fwd is a state-at-a-time search, bwd is
        #                                          the symbolic forward one.
        #   middle      explicit, symfwd, symbwd - symfwd is today's fwd, symbwd today's bwd.
        #   current     fwd, bwd, fwdbwd         - direction is what tells them apart.
        era_current = "fwdbwd_ms" in header
        era_middle = "explicit_ms" in header

        conv_column = header.index("conv") if "conv" in header else None
        profile_column = header.index("profile") if "profile" in header else None
        if conv_column is None or profile_column is None:
            return

        column_engine = {}
        for index, name in enumerate(header):
            if not name.endswith("_ms"):
                continue
            engine = name[: -len("_ms")]
            if era_middle:
                # de-zovl's names for the two that survive.
                engine = {"symfwd": "fwd", "symbwd": "bwd"}.get(engine, engine)
            elif not era_current:
                # The oldest runs. `bwd` was the symbolic forward search and is a usable
                # weight for today's `fwd`. Its `fwd` was a state-at-a-time search that
                # nothing measures now, so it keeps a name nothing asks about rather than
                # poisoning the column that bears that name today.
                engine = {"bwd": "fwd", "fwd": "explicit"}.get(engine, engine)
            column_engine[index] = engine

        for line in lines[1:]:
            cells = line.split(TAB)
            if len(cells) <= max(conv_column, profile_column):
                continue
            conv = cells[conv_column]
            profile = cells[profile_column]
            for index, engine in column_engine.items():
                if index >= len(cells):
                    continue
                cell = cells[index]
                # A crashed or never-run row leaves "?" here, and a "?" is not a duration.
                if not cell.isdigit():
                    continue
                seconds = int(cell) / 1000.0
                self._add((engine, conv, profile), seconds)
                self._add((engine, conv), seconds)
                self._add((engine, profile), seconds)
                self._add((engine,), seconds)
                self._add((), seconds)

    def _mean(self, key):
        n = self.count.get(key, 0)
        return self.total[key] / n if n else 0.0

    @property
    def overhead(self):
        """What a row costs before its engines start, measured where this run can.

        A HAND-SET CONSTANT UNTIL de-12wr.1, and the largest single term in a row's weight:
        on the whole-game run of 2026-09-09 the engines averaged 42 ms of a row that cost
        about 1.4 seconds of wall clock, so weighting a row by its engine time alone weighted
        it by three per cent of itself. That is why the estimate read low, and why it read low
        in the way it did - groups arrive heaviest-first, so the rows already done are the
        ones whose engine time is a real share of their cost and the rows still to come are
        dominated by this constant.

        It could not be measured from the shell: a row was timed by differencing whole
        seconds, which quantises a 1.4-second row to 1 or 2, and the row's own process cannot
        see what spawning it cost. Here every row is timed at the resolution the clock has,
        so the overhead is simply the wall time a row took minus the engine time it recorded,
        averaged over the rows this run measured. Until there is one, the old constant.
        """
        if self.fixed_overhead is not None:
            return self.fixed_overhead
        if self.overhead_count == 0:
            return FALLBACK_ROW_OVERHEAD
        return self.overhead_total / self.overhead_count

    def observe(self, wall_seconds, engine_seconds):
        """One row this run measured: what it took, and what its engines accounted for.

        NEGATIVE IS DISCARDED RATHER THAN CLAMPED. A row whose engines report more time than
        the wall clock saw is a row whose timing is not to be trusted at all - the two clocks
        disagree - and averaging in a zero would quietly pull the overhead down.
        """
        if engine_seconds is None:
            return
        overhead = wall_seconds - engine_seconds
        if overhead < 0:
            return
        self.overhead_total += overhead
        self.overhead_count += 1

    def weight(self, conv, profile):
        """What one row should cost, summed over the engines this run measures."""
        total = self.overhead
        for engine in self.engines:
            for key in (
                (engine, conv, profile),
                (engine, conv),
                (engine, profile),
                (engine,),
                (),
            ):
                if self.count.get(key, 0) > 0:
                    total += self._mean(key)
                    break
        return total

    def estimate(self, done, left):
        """Seconds left, or None where there is nothing to calibrate on.

        `done` is (conv, profile, seconds) for the rows this run has measured; `left` is
        (conv, profile) for the rows still to come.
        """
        # Nothing measured anywhere: the caller's flat mean is the honest answer.
        if self.count.get((), 0) == 0:
            return None

        spent = 0.0
        spent_weight = 0.0
        for conv, profile, seconds in done:
            spent += seconds
            spent_weight += self.weight(conv, profile)

        left_weight = sum(self.weight(conv, profile) for conv, profile in left)

        # No row finished yet, or every row that did weighs nothing, so there is no pace to
        # measure. Nothing left to estimate is not an error either, but it is not a number.
        if spent <= 0 or spent_weight <= 0 or left_weight <= 0:
            return None

        return (spent / spent_weight) * left_weight


###############################################################################
# What one group cost, read back off the rows a run recorded for it
###############################################################################


def group_cost(path):
    """(search_ms, peak_nodes, complete) for one group's TSV.

    READ FROM THE TSV RATHER THAN TIMED BY THE CLOCK, and that is the point. Wall clock is
    not available for a row the resume skipped, and a resumed whole-game run is mostly
    skipped rows - so a split decided on wall clock would see a run of instant groups and
    conclude the cost had bottomed out before it had measured anything at all. What the file
    records is the cost of that group whenever it was measured, which is the same evidence
    either way.

    THE LAST ROW PER PROFILE WINS, because the files are appended to and a retried row sits
    after the one it replaces - the same rule the resume itself reads by.

    `complete` IS FALSE WHERE ANY CELL IS NOT A NUMBER, which is how a group holding a
    CRASHED, NOT-MEASURED or otherwise unfilled row declines to be evidence of anything. A
    group whose rows are all NO-ROWS reports false as well: it measured nothing, so it says
    nothing about whether the measuring has got cheap.
    """
    try:
        lines = Path(path).read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return 0, 0, False
    if not lines:
        return 0, 0, False

    header = lines[0].split(TAB)
    if not header or header[0] != "conv":
        return 0, 0, False

    ms = [i for i, name in enumerate(header) if name.endswith("_ms")]
    held = [i for i, name in enumerate(header) if name.endswith("_nodes")]
    verdict = [i for i, name in enumerate(header) if name.endswith("_verdict")]
    profile_column = header.index("profile") if "profile" in header else None
    if profile_column is None:
        return 0, 0, False

    last = {}
    for line in lines[1:]:
        cells = line.split(TAB)
        if len(cells) <= 1 or len(cells) <= profile_column:
            continue
        last[cells[profile_column]] = cells

    complete = True
    rows = 0
    total_ms = 0
    peak_nodes = 0
    for cells in last.values():
        # A row with nothing in it is not a cost. Skipped rather than counted as zero: a zero
        # would drag the group's total towards the floor and make an empty group look like a
        # cheap one.
        if verdict and all(index < len(cells) and cells[index] == "NO-ROWS" for index in verdict):
            continue
        rows += 1

        for index in ms:
            cell = cells[index] if index < len(cells) else ""
            if cell.isdigit():
                total_ms += int(cell)
            else:
                complete = False
        for index in held:
            cell = cells[index] if index < len(cells) else ""
            if not cell.isdigit():
                complete = False
            else:
                peak_nodes = max(peak_nodes, int(cell))

    if rows == 0:
        complete = False
    return total_ms, peak_nodes, complete


def row_engine_seconds(row, header):
    """What a row's engine columns account for, or None where any of them is not a number."""
    cells = row.split(TAB)
    total = 0.0
    for index, name in enumerate(header):
        if not name.endswith("_ms"):
            continue
        cell = cells[index] if index < len(cells) else ""
        if not cell.isdigit():
            return None
        total += int(cell) / 1000.0
    return total


###############################################################################
# The run
###############################################################################


class Run:
    def __init__(self, conversations):
        self.logs = self._folder()
        self.logs.mkdir(parents=True, exist_ok=True)

        self.profiles = env_list("PROFILES") or list(DEFAULT_PROFILES)

        # EXPORTED ONLY WHEN IT HAS A VALUE. The measurement reads an empty ENGINES as the
        # default, so this is belt and braces - but an empty selection exported into a
        # measurement is the kind of thing that should not have two chances to mean nothing.
        self.child_env = dict(os.environ)
        engines = os.environ.get("ENGINES", "").strip()
        if engines:
            self.child_env["ENGINES"] = engines
        else:
            self.child_env.pop("ENGINES", None)

        # THE CAP EACH ENGINE GETS, so that a row which will never finish still ends. The
        # measurement's own default is ten minutes and this passes whatever is set through
        # unchanged. EACH ENGINE gets it separately, so a row of four columns can spend four.
        self.row_seconds = env_int("ROW_SECONDS", FALLBACK_ROW_SECONDS)
        self.child_env["ROW_SECONDS"] = str(self.row_seconds)

        self.measurement = self._build()
        self.header = self._read_header()
        self.header_fields = self.header.split(TAB)
        self.engine_names = [name[: -len("_verdict")] for name in self.header_fields if name.endswith("_verdict")]
        # WHETHER ANY ENGINE REPORTS WHAT IT HELD. The `_nodes` columns are the manager's own
        # node count, which is memory in use in the currency the budget is spent in - and they
        # are the only evidence the split has that a group would fit a worker's divided share.
        # A run narrowed to a portfolio column alone has no such column, and the split says so
        # rather than assuming the memory is fine because it cannot see any.
        self.reports_nodes = sum(1 for name in self.header_fields if name.endswith("_nodes"))

        constants = self._read_constants()
        self.full_budget_mb = env_int("ROW_MEMORY_MB", constants.get("memory_mb", FALLBACK_MEMORY_MB))
        self.bytes_per_node = constants.get("bytes_per_node", FALLBACK_BYTES_PER_NODE)

        self.conversations = list(conversations)
        self.empty_groups = []

        self.census_file = os.environ.get("CENSUS_FILE", "").strip() or None
        self.census_reused = False
        self.census_reused_from = None
        self.census_repaired = set()

        # Counted so the run can say at the end that part of it is not a measurement. A line
        # scrolled past an hour ago is not a warning.
        self.not_measured = 0
        self.no_rows = 0
        self.no_row_groups = 0
        self.skipped_rows = 0

        # WEIGHTS WITH NO HISTORY, replaced by `read_past_runs` once the past folders have
        # been read. Real rather than None so that every caller can simply use it: a run that
        # never reaches `read_past_runs` - a refusal, say - still has something that answers
        # `estimate` with None and `overhead` with the fallback, which is exactly what a run
        # with no history should get.
        self.weights = RowWeights(self.engine_names, [])
        self.row_done = {}

        self.total_rows = 0
        self.done_rows = 0
        self.started = time.monotonic()

        # The rows this run has measured, as (conv, profile, seconds), and the rows still to
        # come. A LIST OF TUPLES RATHER THAN A `conv:profile=seconds;...` STRING, which is
        # what the shell passed to awk: that was a data structure spelled as text because
        # there was nowhere else to put one.
        self.done_spec = []
        self.left_spec = []

        self.last_estimate = None
        self.last_estimate_at = 0.0
        self.estimate_every = env_int("ESTIMATE_EVERY", 30)

        self.in_parallel = False
        # The parallel phase calibrates on its own rows; see `_group_finished`.
        self.parallel_done_spec = []
        self.parallel_row_seconds = 0.0
        self.parallel_reaped = set()

        # ONE LOCK FOR THE PARENT'S BOOKS. Groups run in threads that each drive their own
        # subprocesses, and everything they share is either a per-group file or a counter
        # folded in here.
        self.lock = threading.Lock()

    # -- setting up -------------------------------------------------------

    def _folder(self):
        """One folder per run, named for the day and the commit it measured.

        The whole matrix is one measurement and its row logs only mean anything as a set.
        Flat files named for the row were overwritten by the next run, which left every
        recorded TSV with no logs behind it except the newest one's.

        THE TSVs GO IN HERE TOO, rather than into measurements/ where they were committed. A
        row is a wall-clock time on one machine and it moves whenever anything about the
        search does, so a committed one is a baseline that is wrong more often than it is
        right - and one that is wrong silently, because nothing re-runs it.

        UNLESS THE RUN NAMES ONE, which is what makes a resume possible: the same folder, the
        same rows, the ones already in it skipped. A generated name cannot be resumed into
        because the next run generates a different one.
        """
        named = os.environ.get("MATRIX_OUT", "").strip()
        if named:
            path = Path(named)
            return path if path.is_absolute() else ROOT / path

        folder = subprocess.run(
            [str(ROOT / "tools" / "run-logged.sh"), "--folder-only", "measure", "matrix"],
            capture_output=True,
            text=True,
            env={**os.environ, "RUN_LOG_DIR": str(OUT / "logs")},
            check=True,
        ).stdout.strip()
        return Path(folder)

    def _build(self):
        """Built once, up front, and then called DIRECTLY rather than through `cargo run`.

        Letting each row build would put a compile inside the timing of whichever row
        happened to run first.

        `cargo run` re-checks the build on every invocation, which is work the build above has
        just done. Measured 2026-09-08 on an up-to-date tree, asking only for the header so
        the measuring itself is nil: 0.552s through cargo against 0.042s for the binary. That
        is ~510ms on every row, and a whole-game run is fourteen thousand of them - about two
        hours spent re-answering one question. It is also a LOCK: concurrent `cargo run`s
        serialise on the target directory, which matters for anything running rows side by
        side.

        CHECKED ONCE, HERE. A missing binary called directly gives an error per row, and every
        one of those would be recorded as a crashed row - a build failure written into the
        folder as fourteen thousand findings.
        """
        print("building...")
        subprocess.run(
            [
                "cargo",
                "build",
                "--release",
                "--example",
                "performance_matrix",
                "--manifest-path",
                str(ROOT / "Cargo.toml"),
            ],
            capture_output=True,
            check=False,
        )

        target = Path(os.environ.get("CARGO_TARGET_DIR") or (ROOT / "target"))
        binary = target / "release" / "examples" / "performance_matrix"
        if not os.access(binary, os.X_OK):
            binary = binary.with_suffix(".exe")
        if not os.access(binary, os.X_OK):
            refuse(f"the measurement did not build - no runnable binary at {binary}", code=1)
        return binary

    def _ask(self, extra_env, timeout=None):
        """One question put to the measurement, answered on stdout."""
        env = dict(self.child_env)
        env.update(extra_env)
        return subprocess.run(
            [str(self.measurement)],
            capture_output=True,
            text=True,
            env=env,
            timeout=timeout,
        )

    def _read_header(self):
        """The column names, ASKED FOR rather than written down.

        They follow the engine selection, and a copy kept here would be wrong for any narrowed
        run and silently wrong for a renamed column - which is the mistake de-zovl exists to
        correct, in the one place it would still be possible to make.
        """
        answer = self._ask({"HEADER_ONLY": "1"})
        for line in answer.stdout.splitlines():
            if line.startswith("conv"):
                return line
        refuse(
            "could not read the column names from the measurement - did the build fail?",
            code=1,
        )

    def _read_constants(self):
        """The numbers the driver does arithmetic with, asked for rather than transcribed.

        The memory budget and the bytes a diagram node costs both live in
        src/symbolic/budget.rs, and the shell driver kept its own copies - "the two numbers
        here that have to be kept in step with the Rust by hand". A hand-kept copy of a
        constant is wrong silently, and this one is wrong in the direction that manufactures
        rows: too large a worker share and the workers race for memory the machine has not
        got.

        AN OLDER BINARY SAYS NOTHING AND IS NOT AN ERROR. It falls back to the figures the
        shell carried, which is exactly where it would have been anyway.
        """
        answer = self._ask({"CONSTANTS_ONLY": "1"})
        constants = {}
        for line in answer.stdout.splitlines():
            name, _, value = line.partition(TAB)
            if value.strip().isdigit():
                constants[name.strip()] = int(value.strip())
        return constants

    # -- which groups -----------------------------------------------------

    def enumerate_groups(self):
        """`all`: ask the measurement which groups exist, and which have anything in them.

        ASKED, NOT LISTED. The whole point of a whole-game run is that nothing decides which
        groups are in it except the index, so the enumeration comes from GROUPS_ONLY - one
        canonical start per distinct closure, heaviest first - and there is no list here to
        fall out of date.

        AND THE ENUMERATION SAYS WHICH GROUPS HAVE ANYTHING IN THEM, which is the fourth
        column and the reason most of a whole-game run no longer happens. 901 of the 1,422
        groups reach nothing from their start - nearly all of them the two-entry ORB stubs the
        database is full of - and measuring one meant ten processes that each read the index,
        built the same graph, found the same nothing and said so. The enumeration answers that
        for every group in the game in about a third of a second, because it has the index open
        already and the question is one walk per group.
        """
        print("asking the measurement which groups exist...")
        answer = self._ask({"GROUPS_ONLY": "1"})

        # KEPT AS WELL AS READ, since de-cziy. The empty groups no longer get a row apiece, so
        # these two files are the whole record of which groups the run considered and why it
        # left some out.
        (self.logs / "groups.log").write_text(answer.stderr, encoding="utf-8")
        (self.logs / "groups.tsv").write_text(answer.stdout, encoding="utf-8")

        found = []
        empty = []
        for line in answer.stdout.splitlines():
            cells = line.split(TAB)
            if not cells or not cells[0].strip():
                continue
            # A MISSING COLUMN IS A STALE BINARY, not an empty group, and the difference is
            # the whole run: read as zero it would prune every group in the game and record
            # the lot as NO-ROWS in seconds. The script and the measurement are built
            # together, so this can only mean the build did not take.
            if len(cells) < 4 or not cells[3].strip():
                refuse(
                    "the measurement's group list has no 'reachable' column - it is older\n"
                    "than this script. Rebuild it and run again.",
                    code=1,
                )
            start = int(cells[0])
            if int(cells[3]) > 0:
                found.append(start)
            else:
                empty.append(start)

        if not found and not empty:
            refuse("the measurement listed no groups - did the index read?", code=1)

        print(f"{len(found) + len(empty)} groups, {len(found)} of them with rows")
        self.conversations = found
        self.empty_groups = empty

    # -- the census -------------------------------------------------------

    def arrange_census(self):
        """A census, if the grid needs one and there is none.

        The unreachable profiles read a census to know which entries no path can reach. They
        are in the default grid now, so the ordinary command has to be able to produce one -
        otherwise a plain run on a fresh clone stops before its first row, which is exactly
        the objection that kept those profiles out of the grid.

        INTO THE RUN'S OWN FOLDER, beside the rows drawn from it. That is the same rule the
        TSVs follow and for the same reason: a census is an artefact a row depends on, and one
        kept somewhere else is one nobody can find when the row is read a month later.

        BEFORE THE FIRST ROW, so its cost lands nowhere near a row's clock.

        ONLY WHEN THERE IS NOTHING TO READ. A named CENSUS_FILE is used exactly as given and
        NOTHING HERE CHECKS WHETHER IT IS CURRENT - not its age, not which groups it covers,
        not the world it was taken under. Deciding a census is stale needs a rule for what
        stale means, and a wrong rule silently re-takes a census somebody deliberately
        supplied, or silently keeps one it should not. The measurement already shouts when a
        census and a search contradict each other, which is the check that can actually be
        made.
        """
        needs = any(name.startswith("deepest-unreach-") for name in self.profiles)
        if not needs or self.census_file:
            if self.census_file:
                self.child_env["CENSUS_FILE"] = self.census_file
            return

        self.census_file = str(self.logs / "census" / "census.tsv")
        if Path(self.census_file).exists():
            print(f"using the census this folder already holds: {self.census_file}")
            self.child_env["CENSUS_FILE"] = self.census_file
            return

        # A CENSUS FROM AN EARLIER RUN, ASSUMED GOOD AND REPAIRED WHERE IT IS NOT.
        #
        # Taking one costs about eight minutes on the whole game, which is a third of the run,
        # and two runs an hour apart produced byte-identical censuses on all 521 groups - so
        # the common case is paying a third of a run to reproduce an answer already on disk.
        #
        # WHY THIS IS SAFE WITHOUT A STALENESS RULE. A census that is wrong about a group makes
        # that group's unreachable profiles ask about entries that are not unreachable, and the
        # measurement ALREADY CATCHES THAT: a `found` on a deepest-unreach profile is
        # impossible by construction, and it prints CONTRADICTION when it happens. So the run
        # does not have to predict staleness, it can detect it - and being wrong costs one
        # group's census and one group's unreachable rows rather than the run's correctness.
        #
        # WHAT IT DOES NOT CATCH is a census that named too FEW unreachable entries: the
        # profile is built from that shorter list, so every row in it is answerable and nothing
        # contradicts. That is the residual risk of assuming a census is good, and it is why
        # the reuse says so loudly rather than quietly.
        #
        # COPIED IN RATHER THAN READ IN PLACE. The rows about to be written depend on it, so it
        # belongs beside them - the same rule the TSVs follow. It also means the repair edits
        # this run's copy and not the record of the run it came from.
        reuse_from = None
        for folder in self._folders_newest_first():
            if folder == self.logs:
                continue
            candidate = folder / "census" / "census.tsv"
            if candidate.is_file() and candidate.stat().st_size > 0:
                reuse_from = candidate
                break

        if reuse_from is not None and os.environ.get("CENSUS_REUSE", "yes") == "yes":
            (self.logs / "census").mkdir(parents=True, exist_ok=True)
            shutil.copyfile(reuse_from, self.census_file)
            self.census_reused = True
            self.census_reused_from = reuse_from
            print(f"REUSING a census rather than taking one: {reuse_from}")
            print(f"  copied to {self.census_file}. It is assumed good; a group whose rows contradict")
            print("  it is re-censused and re-measured. CENSUS_REUSE=no takes a fresh one.")
        else:
            print("taking a census first - the grid has an unreachable profile and none was named")
            # THE SAME GROUPS THE ROWS WILL ASK ABOUT, named rather than `all`. A whole-game
            # census over 1,422 groups to serve a run of six is hours spent on rows nobody
            # asked for.
            subprocess.run(
                [str(ROOT / "tools" / "measure-census.sh")] + [str(c) for c in self.conversations],
                env={**os.environ, "CENSUS_OUT": str(self.logs / "census")},
                check=False,
            )
            if not Path(self.census_file).exists():
                refuse(
                    f"the census produced no {self.census_file} - stopping rather than "
                    "measuring\nrows against a census that is not there.",
                    code=1,
                )

        self.child_env["CENSUS_FILE"] = self.census_file

    def _folders_newest_first(self):
        try:
            folders = [p for p in (OUT / "logs").iterdir() if p.is_dir()]
        except OSError:
            return []
        return sorted(folders, key=lambda p: p.stat().st_mtime, reverse=True)

    def read_past_runs(self):
        """What past runs cost, for the weighted estimate.

        This run's own folder is excluded: its rows are the ones being calibrated, and letting
        them weigh themselves would drag every ratio towards one as the run went on.

        THE LAST THREE RUNS, NOT EVERY RUN EVER KEPT. Every folder under measurements/logs used
        to feed this, which is 4,020 TSVs and grows with each run - so each run made the next
        one's estimate slower, and the estimate is recomputed on a timer. Two whole-game
        folders of 1,422 files each were most of that weight and were measured before several
        changes to what a row costs, so the bulk was also the stale part.

        NEWEST FIRST BY MODIFICATION TIME rather than by name, because not every folder is
        date-stamped and a resumed run is genuinely more recent than its name says. A folder
        with no matrix TSVs in it does not count as one of the three.
        """
        wanted = env_int("PAST_RUNS", 3)
        tsvs = []
        kept = 0
        for folder in self._folders_newest_first():
            if kept >= wanted:
                break
            if folder == self.logs:
                continue
            here = sorted(folder.glob("performance-matrix-*.tsv"))
            if not here:
                continue
            tsvs.extend(here)
            kept += 1

        overhead = os.environ.get("ROW_OVERHEAD", "").strip()
        self.weights = RowWeights(self.engine_names, tsvs, float(overhead) if overhead else None)

    def read_folder(self):
        """What this folder already holds, which is the whole of the resume.

        A row is done if the folder has a line for it that is not NOT-MEASURED - see the module
        docstring for why that one outcome is the exception. THE LAST LINE PER KEY WINS,
        because the files are appended to and a retried row sits after the one it replaces.

        READ BEFORE ANYTHING IS WRITTEN, which the pruning depends on: a group recorded as
        NO-ROWS on the last run must not have ten more NO-ROWS rows appended to it on this one.
        """
        for tsv in sorted(self.logs.glob("performance-matrix-*.tsv")):
            for line in tsv.read_text(encoding="utf-8", errors="replace").splitlines():
                cells = line.split(TAB)
                if len(cells) < 3 or cells[0] in ("conv", ""):
                    continue
                key = (cells[0], cells[2])
                if "NOT-MEASURED" in TAB.join(cells[3:]):
                    self.row_done.pop(key, None)
                else:
                    self.row_done[key] = True

    # -- rows -------------------------------------------------------------

    def verdict_row(self, conversation, profile, verdict):
        """A row with no measurement in it, shaped by the header.

        THREE CALLERS, AND THEY MEAN DIFFERENT THINGS - CRASHED, the row took the process
        down; NO-ROWS, a row process looked and said there was nothing here; and NO-ROWS again
        for a group the enumeration pruned before any process ran. Sharing the shaping and not
        the verdict is what keeps them one line apart in the file.

        The loop is over the header's fields, so it still follows a narrowed run's columns
        rather than assuming a shape.
        """
        cells = []
        for name in self.header_fields:
            if name == "conv":
                cells.append(str(conversation))
            elif name == "profile":
                cells.append(profile)
            elif name.endswith("_verdict"):
                cells.append(verdict)
            else:
                cells.append("?")
        return TAB.join(cells)

    def measure_group(self, conversation, wanted=None, redo=False, in_repair=False):
        """One group, start to finish: one process per row.

        IT COUNTS ITS OWN OUTCOMES and hands them back, because in the parallel phase it runs
        in a thread and the parent folds them in when the group is reaped. In the shell this
        was a fork and a sourced stat file; here it is a return value, which is one fewer
        thing to be lost when a worker dies.
        """
        wanted = list(wanted) if wanted else list(self.profiles)
        tally = {
            "not_measured": 0,
            "no_rows": 0,
            "skipped": 0,
            "rows": 0,
            "done_spec": [],
        }
        out = []

        def say(text=""):
            out.append(text)

        tsv = self.logs / f"performance-matrix-{conversation}.tsv"
        # ONLY WHEN THE FILE IS NEW. Truncating it here is what a resume must not do, and the
        # header is the one line that would otherwise be written twice.
        if not tsv.exists():
            tsv.write_text(self.header + "\n", encoding="utf-8")
        say(f"=== {conversation} -> {tsv}")

        for profile in wanted:
            log = self.logs / f"matrix-{conversation}-{profile}.log"
            key = (str(conversation), profile)

            # ALREADY ANSWERED, so not asked again. Counted as done for the progress line,
            # since what the run has left is what it has left however the rows got there.
            #
            # A REPAIR RE-ASKS A ROW THE FOLDER ALREADY HOLDS, which is the one case where
            # `already measured` is the wrong answer: the row is there and it was measured
            # against a census since replaced. The appended row wins, by the same
            # last-row-per-key rule the resume reads by, so nothing has to be deleted first.
            if not redo and self.row_done.get(key):
                # NO CLOCK, DELIBERATELY: nothing ran, so there is no start to report. The
                # space where one would go is held open so this line stays in the same column
                # as the rows that did run.
                say(f"  {'':8}  {profile:<18} already measured")
                tally["skipped"] += 1
                tally["rows"] += 1
                if not in_repair and self._row_finished(key, None, "\n".join(out)):
                    # THE BUFFER IS CLEARED ONLY WHERE IT WAS PRINTED. In the serial phase it
                    # goes out with the row, so holding it back would print a resumed group's
                    # lines after the progress line that already counted them. In the parallel
                    # phase nothing is printed per row, so clearing here would throw the
                    # group's whole block away - which is what it did, silently, until the
                    # first parallel run showed group progress lines and no groups.
                    out = []
                continue

            row_started = time.monotonic()
            row_clock = time.strftime("%H:%M:%S")

            # THE WALL CLOCK IS ON THIS LINE, THE ONE THAT EXISTS WHILE THE ROW IS RUNNING.
            # de-p58a. The progress line carries durations only - all relative to a start
            # nobody wrote down - so a reader could not say when a row began and, for the row
            # in flight, could not say anything at all.
            say(f"  {row_clock}  {profile:<18} ...")

            status = self._run_row(conversation, profile, log)
            took = time.monotonic() - row_started

            # THE SEPARATOR IS PRINTED, NOT IMPLIED. de-qm7a: this used to let the field
            # padding supply the gap before the verdict, and "deepest-unreach-1" at seventeen
            # characters overflowed a twelve-wide field, so the log said
            # "deepest-unreach-1ok". The width is for ALIGNMENT and the trailing space is for
            # correctness.
            prefix = f"  {row_clock}  {profile:<18} "

            text = log.read_text(encoding="utf-8", errors="replace")

            # EXIT 2 IS "YOU ASKED FOR SOMETHING THAT DOES NOT EXIST", and it stops the run.
            # The measurement refuses an unknown profile or a conversation id that is not one,
            # rather than quietly selecting nothing (de-uxyw). Without this the refusal looked
            # exactly like a crash - no row line in the log - so a mistyped sixty-six row run
            # produced sixty-six CRASHED rows and took its several seconds over each of them.
            if status == 2:
                say(prefix + "REFUSED")
                for line in text.splitlines():
                    say("  " + line)
                say()
                print("\n".join(out))
                refuse("nothing was measured; fix the selection and run again")

            row = next(
                (line for line in text.splitlines() if re.match(rf"^{conversation}\b", line)),
                None,
            )
            if row:
                with self.lock:
                    with tsv.open("a", encoding="utf-8") as handle:
                        handle.write(row + "\n")

                # THREE OUTCOMES, NOT TWO, and the third is not a result. The measurement
                # prints a NOT-MEASURED row when the machine could not supply the budget; that
                # says nothing about the search and the run wants repeating with the memory
                # free. Flattening it in with the real rows is how a gap gets read as a
                # finding.
                if "NOT-MEASURED" in row:
                    say(prefix + "NOT MEASURED - no memory for the budget; rerun this row")
                    tally["not_measured"] += 1
                else:
                    say(prefix + "ok")
                    # WHAT THE ROW COST BEYOND ITS ENGINES, which is what makes ROW_OVERHEAD a
                    # measurement rather than a constant. Only from a row that measured
                    # something: a NOT-MEASURED row never ran its engines.
                    self.weights.observe(took, row_engine_seconds(row, self.header_fields))
            elif re.search(r"; no rows$", text, re.MULTILINE):
                # ASKING FOR A GROUP WITH NO ROWS IS AN ERROR, and it stops the run. From the
                # user, de-cziy: no-row groups are to be skipped, and naming one is a mistake
                # worth hearing about rather than a row to file.
                #
                # THIS IS ONLY REACHABLE FROM A NAMED RUN. A whole-game run prunes the empty
                # groups from the enumeration before any process starts, so the row loop never
                # meets one - unless a person typed it.
                #
                # AND IT CATCHES A SECOND THING. The row loop and the enumeration ask the same
                # function whether a group has rows, and if they ever drift, `all` will reach
                # here too and say so instead of quietly recording a row.
                said = next(line for line in text.splitlines() if line.endswith("; no rows"))
                say(prefix + "REFUSED")
                say(f"  {said}")
                say()

                # THE HEADER-ONLY FILE GOES WITH IT. The tsv is created before the first row
                # is attempted, so a refusal that left it behind would put an empty stub in
                # the folder for a group that is meant not to appear at all. Only if it is
                # still just the header: a group that measured something earlier and is being
                # re-run keeps what it has.
                if len(tsv.read_text(encoding="utf-8").splitlines()) <= 1:
                    tsv.unlink(missing_ok=True)

                print("\n".join(out))
                refuse(
                    f"conversation {conversation} has no rows to measure, so naming it asks\n"
                    "for something that does not exist. Drop it from the list and run again;\n"
                    "a whole-game run skips such groups by itself."
                )
            else:
                # A CRASH IS A RESULT. The row says so and names its log, rather than being
                # silently absent - an empty line in a measurement reads as "not run yet",
                # which is a different thing from "this is what happens". Distinct from
                # NOT-MEASURED above: this row died, that one never ran. And distinct from
                # NO-ROWS: that one looked and found nothing, this one never came back.
                with self.lock:
                    with tsv.open("a", encoding="utf-8") as handle:
                        handle.write(self.verdict_row(conversation, profile, "CRASHED") + "\n")
                say(prefix + f"CRASHED (see {log})")

            tally["rows"] += 1
            tally["done_spec"].append((str(conversation), profile, took))
            if not in_repair and self._row_finished(key, took, "\n".join(out)):
                out = []

        if out:
            tally["output"] = "\n".join(out)
        else:
            tally["output"] = ""
        return tally

    def _run_row(self, conversation, profile, log):
        """One row, in its own process, tee'd to its log and watched on stdout.

        THE ROW LOG GETS EVERYTHING; THE RUN LOG GETS THE PROGRESS LINES. A heavy row is half
        an hour inside one process, and redirecting it wholesale to its own file meant the only
        thing being watched said nothing for half an hour while the interesting lines went
        somewhere nobody was looking. Whether a row is watchable and whether its log is
        complete are not the same question, so both are answered: the file keeps the whole
        output, and only the lines the measurement marks with the progress prefix reach stdout.

        CREATED EMPTY FIRST, because a row can produce NO output at all and the CRASHED
        message would otherwise name a file that is not there. An empty log is the honest
        artefact of a row that said nothing.
        """
        env = dict(self.child_env)
        env["CONVERSATION"] = str(conversation)
        env["PROFILE"] = profile
        env["NO_HEADER"] = "1"

        with log.open("w", encoding="utf-8") as handle:
            process = subprocess.Popen(
                [str(self.measurement)],
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                errors="replace",
                env=env,
                bufsize=1,
            )
            # PIPED, so there is a stream; the assertion is for the reader and the checker
            # rather than for a case that can happen.
            assert process.stdout is not None
            for line in process.stdout:
                handle.write(line)
                handle.flush()
                if line.startswith("  ~"):
                    print(line.rstrip("\n"), flush=True)
            return process.wait()

    # -- the progress line ------------------------------------------------

    def _row_finished(self, key, took, output=""):
        """One row is done. Returns whether it printed the caller's buffered output.

        In the serial phase that is the progress line and its weighted estimate. In the
        parallel phase there is no line to print - with four rows in flight, "row 0:00:02" is
        not a duration anybody waited - so the group's tally carries the durations and the
        parent reports by GROUP when it reaps one, printing the group's block whole so that
        four groups at once do not interleave into something nobody can read.

        THE RETURN VALUE IS WHAT TELLS THE CALLER WHETHER TO CLEAR ITS BUFFER, and it is not
        bookkeeping: clearing it unconditionally discards every parallel group's output, which
        is a run that reports progress and never says which groups it measured.

        A SKIPPED ROW CONTRIBUTES NOTHING to the pace: it advances the run without costing it
        anything, and feeding it in as zero seconds against its full weight drags the pace
        down. On a resumed whole-game run those are most of the rows.
        """
        if self.in_parallel:
            return False

        if output:
            print(output, flush=True)

        self.done_rows += 1
        if took is None:
            self.skipped_rows += 1
        else:
            self.done_spec.append((key[0], key[1], took))
        if self.left_spec:
            self.left_spec.pop(0)

        now = time.monotonic()
        elapsed = now - self.started
        left = self.total_rows - self.done_rows

        estimate, note = self._estimate(left, self.done_spec, self.left_spec, now)

        print(
            f"    {self.done_rows}/{self.total_rows} "
            f"({self.done_rows * 100 // max(1, self.total_rows)}%)  "
            f"row {clock(took or 0)}  elapsed {clock(elapsed)}  "
            f"est. left ~{clock(estimate)}{note}",
            flush=True,
        )
        return True

    def _estimate(self, left, done_spec, left_spec, now):
        """The weighted estimate, throttled and counted down, or the flat mean.

        HOW OFTEN IT IS RECOMPUTED. Every ESTIMATE_EVERY seconds at most, because it re-reads
        every past TSV and is handed a list naming every row still to come: at fourteen
        thousand rows that is real time. Between refreshes the last figure is reprinted.

        COUNTED DOWN SINCE IT WAS TAKEN. Reprinting it unchanged made it read as frozen, then
        JUMP UP when the refresh replaced a figure that had quietly gone stale. Subtracting the
        age costs nothing and is what a reader expects a countdown to do. Floored at zero,
        because an estimate that ran out is late rather than negative.

        THE FLAT MEAN IS THE FALLBACK and says so when it is used. It reads LONG early on,
        because each conversation's heavy profiles run first, and that is the whole reason the
        weights are worth having. It is taken over what this run actually SPENT, not over its
        elapsed time: a resumed run's elapsed clock includes rows it skipped in no time at all.
        """
        if left <= 0:
            return 0, ""

        should_refresh = (
            done_spec
            and left_spec
            and (self.last_estimate is None or now - self.last_estimate_at >= self.estimate_every)
        )
        if should_refresh:
            weighted = self.weights.estimate(done_spec, [(conv, profile) for conv, profile, *_ in left_spec])
            if weighted is not None:
                self.last_estimate = weighted
            self.last_estimate_at = now

        if self.last_estimate is not None:
            return max(0.0, self.last_estimate - (now - self.last_estimate_at)), ""

        spent = sum(seconds for _, _, seconds in done_spec)
        measured = len(done_spec)
        if measured > 0:
            return spent * left / measured, " (flat)"
        return 0, " (flat)"

    # -- the census contradiction -----------------------------------------

    def contradicted(self, conversation):
        """Whether this group's rows contradicted the census.

        The measurement says so itself, on stderr, and the row logs hold stderr: a `found` on
        a deepest-unreach profile is impossible, because every entry in that set was PROVED
        unreachable, so finding one means the census and the search disagree. `CONTRADICTION:`
        is the interface between the two halves - whichever end changes the word changes both.
        """
        for profile in self.profiles:
            if not profile.startswith("deepest-unreach-"):
                continue
            log = self.logs / f"matrix-{conversation}-{profile}.log"
            try:
                text = log.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            if re.search(r"^CONTRADICTION:", text, re.MULTILINE):
                return True
        return False

    def repair_census_for(self, conversation):
        """Re-census one group and re-measure what read the census.

        Only for a census this run did NOT take. One this run took contradicting its own rows
        is a fault in the census or the search, and repairing it would loop while hiding that.

        ONCE PER GROUP. If a freshly taken census contradicts again, staleness was not the
        cause: say so and leave the row as it came, because a repair that can run twice can run
        for ever.

        THE CENSUS ROW IS APPENDED, NOT REWRITTEN. measure-census.sh and the measurement both
        read the LAST row per conversation, so the fresh row simply sits after the stale one -
        the same rule that makes the census resumable.
        """
        # A REUSED CENSUS IS A NAMED FILE, always: `arrange_census` sets `census_file` before
        # it can set `census_reused`, so the second implies the first.
        if not self.census_reused or self.census_file is None:
            print(f"  CONTRADICTION on {conversation} against a census THIS RUN TOOK - not repairing.")
            print("  That is a fault in the census or the search rather than a stale file.")
            return
        if conversation in self.census_repaired:
            print(f"  {conversation} contradicts a census taken for it moments ago - not repairing again.")
            return
        self.census_repaired.add(conversation)

        print(f"  REPAIRING {conversation}: its rows contradict the reused census, so re-censusing")
        print("  that one group and re-measuring the profiles that read it.")

        fresh = self.logs / "census" / f"repair-{conversation}.log"
        fresh.parent.mkdir(parents=True, exist_ok=True)
        answer = self._ask(
            {
                "CENSUS": "1",
                "NO_HEADER": "1",
                "CONVERSATION": str(conversation),
                "CENSUS_JOURNAL": str(self.logs / "census" / f"{conversation}.repair.journal.tsv"),
            }
        )
        fresh.write_text(answer.stdout + answer.stderr, encoding="utf-8")
        if answer.returncode != 0:
            print(f"  the re-census of {conversation} failed - see {fresh}. Leaving its rows as measured.")
            return

        rows = [line for line in answer.stdout.splitlines() if line.startswith(f"{conversation}{TAB}")]
        if not rows:
            print(f"  the re-census of {conversation} produced no row - see {fresh}.")
            return
        with open(self.census_file, "a", encoding="utf-8") as handle:
            handle.write(rows[-1] + "\n")

        reads_census = [p for p in self.profiles if p.startswith("deepest-unreach-")]
        if not reads_census:
            return

        # A REPAIRED ROW IS NOT ONE OF THE RUN'S ROWS. The run planned `total_rows` of them,
        # so counting an unplanned re-measurement would walk the progress line off the end of
        # its own list. The row is re-measured, its result appended and its verdict what the
        # run reports; only the accounting ignores it.
        tally = self.measure_group(conversation, wanted=reads_census, redo=True, in_repair=True)
        if tally["output"]:
            print(tally["output"], flush=True)


###############################################################################
# The phases
###############################################################################


def split_message(run, workers, serial_groups, fits_nodes, worker_nodes, worker_mb):
    """What the run is watching for, said before it starts watching.

    WHY THE SPLIT CANNOT BE DECIDED UP FRONT, and what is lost by that: the run can no longer
    say at the start how many groups go each way. It says what it is watching for instead, and
    says the moment it switches.
    """
    settle_groups = env_int("SETTLE_GROUPS", 10)
    settle_factor = env_int("SETTLE_FACTOR", 2)
    settle_ms = env_int("SETTLE_MS", 2500)
    headroom = env_int("MEMORY_HEADROOM", 2)

    if workers <= 1:
        print("one worker: every group one at a time")
    elif serial_groups:
        print(f"{serial_groups} group(s) one at a time, then {workers} at a time (SERIAL_GROUPS was set)")
    elif run.reports_nodes == 0:
        # No `_nodes` column, so nothing here can say what a group held, and clearing a group
        # for a quarter of the budget on no evidence is exactly the mistake the headroom
        # exists to avoid. Refusing to switch is slow; switching blind manufactures rows.
        print("no engine in this selection reports nodes held, so nothing can say whether a group")
        print("would fit a worker's share of the budget: every group one at a time. Name")
        print("SERIAL_GROUPS=n to split anyway.")
    else:
        cheap_says = f"within {settle_factor}x the cheapest group so far"
        if settle_ms > 0:
            cheap_says += f" or under {settle_ms}ms"
        print(
            f"one group at a time until the cost bottoms out: {settle_groups} in a row "
            f"{cheap_says}, each holding at most {fits_nodes} nodes"
        )
        print(
            f"  (1/{headroom} of the {worker_nodes} a worker's {worker_mb} MB share of the "
            f"{run.full_budget_mb} MB budget buys)"
        )

    return settle_groups, settle_factor, settle_ms


def serial_phase(run, workers, serial_groups, fits_nodes, settle):
    """The heavy groups one at a time, and the run decides where that ends.

    It used to be a number: the first twenty-five groups serially, because on the run of
    2026-09-07 the last row to take more than ten seconds was in group 26. That number is a
    property of one measurement of one index on one machine. Every one of those can move - a
    change to the search, a group that grows entries, a machine with more cores and so a
    smaller share of the budget each - and when it does the constant is silently in the wrong
    place, in the direction that matters: a heavy group measured in parallel gets a DIVIDED
    budget and a contended clock, which is a row that looks like a finding and is an artefact.

    So the run watches two metrics per group and switches when both say the tail has arrived.

    1. THE TIME HAS BOTTOMED OUT, or is simply small. Groups arrive heaviest-first, so the cost
       falls and then flattens, and a group counts as settled when its recorded search time is
       within SETTLE_FACTOR of the run's own cheapest group so far - OR under SETTLE_MS
       outright.

       TWO ARMS BECAUSE THE TWO REGIMES ARE NOT ALIKE. Under ENGINES=all a group can cost
       minutes, no group would pass an absolute two and a half seconds, and the relative arm is
       the only one that can say anything. The default grid has the opposite shape: both its
       engines are walled by the player's own time budget, so every row returns in about two
       seconds whatever the group and the floor falls to about 120 ms - at which point twice
       the floor is 240 ms and a group costing 741 ms resets the count for being three times a
       number that is itself nothing. The run of 2026-09-08 measured 74 of its 100 groups one
       at a time on that reasoning, with cores idle throughout. So the absolute arm is a floor
       under the relative one rather than a replacement for it.

    2. WHAT IT HELD FITS THE CAP A WORKER WILL GET, comfortably. Each parallel worker is
       allowed the budget divided by the worker count, so the question is not "did this fit six
       gigabytes" but "would it have fitted a quarter of them" - and with MEMORY_HEADROOM to
       spare, because the groups being cleared for are the ones AFTER this one, which nothing
       has measured yet.

    BOTH, FOR SETTLE_GROUPS GROUPS IN A ROW, and one that fails either resets the count. The
    curve is not monotone: over the whole game the ten groups after the seven heavy ones look
    like the tail, and then 825, 362, 1030 and 625 arrive - the last of them 47s and a
    gigabyte, at group 26. A window of one would have handed all four to the workers; ten in a
    row does not switch until group 35, after which the heaviest thing left in the game is 15s
    and 33 MB, or two per cent of a worker's cap.
    """
    settle_groups, settle_factor, settle_ms = settle

    serial_done = 0
    floor_ms = None
    settled = 0
    window_max_ms = 0
    window_max_nodes = 0

    for conversation in run.conversations:
        tally = run.measure_group(conversation)
        if tally["output"]:
            print(tally["output"], flush=True)
        if run.contradicted(conversation):
            run.repair_census_for(conversation)
        run.not_measured += tally["not_measured"]
        run.no_rows += tally["no_rows"]
        serial_done += 1

        if workers <= 1:
            continue
        if serial_groups:
            if serial_done >= serial_groups:
                break
            continue
        if run.reports_nodes == 0:
            continue

        # WHAT THIS GROUP COST, off its own file, so a group the resume skipped still counts.
        group_ms, group_nodes, complete = group_cost(run.logs / f"performance-matrix-{conversation}.tsv")

        if not complete:
            # A group with a crashed, unmeasured or empty row in it is not evidence that the
            # measuring has got cheap - it is evidence that something did not measure.
            settled = 0
            window_max_ms = 0
            window_max_nodes = 0
            continue

        if floor_ms is None or group_ms < floor_ms:
            floor_ms = group_ms

        # CHEAP EITHER WAY ROUND - near the run's own floor, or small enough that the floor
        # does not matter. The memory test is unchanged and still has to pass: time alone has
        # never been what makes the switch safe.
        cheap = group_ms <= floor_ms * settle_factor
        if settle_ms > 0 and group_ms <= settle_ms:
            cheap = True

        if cheap and group_nodes <= fits_nodes:
            settled += 1
            window_max_ms = max(window_max_ms, group_ms)
            window_max_nodes = max(window_max_nodes, group_nodes)
        else:
            settled = 0
            window_max_ms = 0
            window_max_nodes = 0

        if settled >= settle_groups:
            print(
                f"  cost has bottomed out after {serial_done} group(s): the last "
                f"{settle_groups} spent at most {window_max_ms}ms of search against a floor "
                f"of {floor_ms}ms,"
            )
            print(
                f"  and held at most {window_max_nodes} nodes of the worker's share. The rest go {workers} at a time."
            )
            break

    return serial_done


def parallel_phase(run, groups, workers, worker_mb):
    """The tail, several groups at a time.

    GROUPS RUN IN PARALLEL, NEVER ROWS, so no two workers ever touch one file: a group owns its
    performance-matrix-<start>.tsv. The append-as-it-finishes resume needs no locking and no
    changes, and rows within a group stay in their own order.

    EACH WORKER GETS ITS SHARE OF THE ALLOWANCE, and this is a correctness fix rather than
    tidiness. The manager PREALLOCATES about two thirds of the budget up front and cannot grow
    past it, so a 43-entry tail group commits roughly four gigabytes exactly as a heavy one
    does - nothing about a small group makes it cheaper. Four workers at the 6 GB default would
    commit ~16 GB before measuring anything, and each one's probe fallibly reserves the FULL 6
    GB first. Overlap those and the run manufactures NOT-MEASURED rows where a probe was
    refused and CRASHED rows where a probe passed and the allocation aborted.

    THE FOLDER IS TOLD WHICH ALLOWANCE THESE ROWS GOT. Two rows given different budgets are not
    comparable and nothing in a TSV records the budget, so it is written down here instead. A
    `no-room` row measured under a divided budget is SUSPECT and wants re-running serially at
    the full allowance before it is believed.
    """
    from concurrent.futures import ThreadPoolExecutor

    run.in_parallel = True
    started = time.monotonic()

    # HOW MANY OF THESE GROUPS WILL COST ANYTHING, counted once and up front.
    #
    # de-x8ms.2. The estimate used to be elapsed * groups_left / groups_done, over ALL groups -
    # and on a resume most of them are rows the folder already holds, which advance groups_done
    # in no time at all. The apparent pace goes through the roof and the estimate collapses,
    # and it does so worst exactly when it is most looked at, because a resume is the normal
    # way this is run.
    #
    # COUNTED HERE RATHER THAN AS GROUPS FINISH, because groups finish out of order when
    # several run at once, so there is no prefix of the list to ask about.
    groups_with_work = sum(
        1
        for conversation in groups
        if any(not run.row_done.get((str(conversation), profile)) for profile in run.profiles)
    )

    run.child_env["ROW_MEMORY_MB"] = str(worker_mb)
    print(f"each worker is allowed {worker_mb} MB of the {run.full_budget_mb} MB budget")
    with (run.logs / "parallel-phase.txt").open("a", encoding="utf-8") as handle:
        handle.write(
            f"workers={workers} budget_mb={worker_mb} of={run.full_budget_mb} "
            f"groups={len(groups)} started={time.strftime('%F %T')}\n"
        )

    # THE PARALLEL PHASE CALIBRATES ON ITS OWN ROWS, so it starts with none rather than with
    # the serial phase's - they were measured one at a time on an uncontended machine at the
    # full budget, and these run several at a time on a fraction of it. Two paces averaged is
    # neither. `last_estimate` is cleared with them: it holds a serial figure.
    run.last_estimate = None
    run.last_estimate_at = 0.0

    state = {"done": 0, "measured": 0}

    def left_rows():
        """The rows of every group not yet reaped, which is what the run still has to pay for.

        A GROUP IN FLIGHT COUNTS AS ENTIRELY UNPAID, including the rows it has already
        finished, because their durations do not reach the parent until the group is reaped.
        That reads slightly long and corrects itself at the next reap, which is the safe
        direction for a number somebody is deciding whether to wait for.
        """
        rows = []
        for conversation in groups:
            if conversation in run.parallel_reaped:
                continue
            for profile in run.profiles:
                if not run.row_done.get((str(conversation), profile)):
                    rows.append((str(conversation), profile))
        return rows

    def reap(conversation, tally):
        """One group is finished: print what it said, fold its tally in, say where the run is.

        WEIGHTED BY WHAT EACH REMAINING ROW HAS COST BEFORE, the same way the serial phase's
        line is. What made this hard in the shell is that a parallel row's duration was known
        only inside the fork that ran it; the parent saw a tally and a group's wall time and
        nothing else. Here the durations come back with the tally.

        DIVIDED BY THE CONCURRENCY THIS RUN IS ACTUALLY ACHIEVING, not by the worker count.
        Dividing by the worker count says every worker will be busy every second of what is
        left, and none of them is: groups differ by orders of magnitude in what they cost so
        slots sit empty, and the run DRAINS at the end with fewer groups left than workers to
        give them to. Measured on the whole-game run of 2026-09-09 that made the figure read
        between a half and four fifths of the truth, worsening from 0.67 at the start to 0.50
        at the end - the drain, arriving on schedule.

        The parent already holds both numbers the real figure needs: the wall time since the
        phase began, and the row-seconds folded in from the groups it has reaped. Their ratio
        IS the speed-up, contention and idle slots included.

        IT READS HIGH EARLY, DELIBERATELY. The clock starts before the first group is reaped,
        so the first few readings divide by a concurrency that has not had time to happen.
        Over-reading is the safe direction for a number somebody is deciding whether to wait
        for, and it corrects itself within a group or two.
        """
        if tally["output"]:
            print(tally["output"], flush=True)

        # REAPED BEFORE ITS ROWS ARE FOLDED IN, so `left_rows` stops counting this group's rows
        # as still to come at the same moment they start counting as done.
        run.parallel_reaped.add(conversation)
        run.not_measured += tally["not_measured"]
        run.no_rows += tally["no_rows"]
        run.skipped_rows += tally["skipped"]
        run.parallel_done_spec.extend(tally["done_spec"])
        run.parallel_row_seconds += sum(seconds for _, _, seconds in tally["done_spec"])

        # REPAIRED IN THE PARENT, AFTER REAPING, and never inside a worker. The census is one
        # file the whole run reads, so several workers repairing at once would append to it
        # concurrently and re-measure against a file still being written. Repairs are rare and
        # a serial one costs only itself.
        if run.contradicted(conversation):
            run.repair_census_for(conversation)

        # EVERY ROW ALREADY THERE means the group cost this run nothing, so it must not
        # calibrate the pace. `rows` counts every row the group had, skipped ones included,
        # which is why the comparison is against it rather than a zero test.
        skipped_entirely = tally["rows"] > 0 and tally["skipped"] >= tally["rows"]

        state["done"] += 1
        if not skipped_entirely:
            state["measured"] += 1

        now = time.monotonic()
        elapsed = now - started
        left = groups_with_work - state["measured"]

        note = ""
        if left <= 0:
            estimate = 0.0
        else:
            should_refresh = run.parallel_done_spec and (
                run.last_estimate is None or now - run.last_estimate_at >= run.estimate_every
            )
            if should_refresh:
                rows = left_rows()
                if rows:
                    weighted = run.weights.estimate(run.parallel_done_spec, rows)
                    if weighted is not None:
                        if run.parallel_row_seconds > 0 and elapsed > 0:
                            run.last_estimate = weighted * elapsed / run.parallel_row_seconds
                        else:
                            run.last_estimate = weighted / workers
                    run.last_estimate_at = now

            if run.last_estimate is not None:
                estimate = max(0.0, run.last_estimate - (now - run.last_estimate_at))
            elif state["measured"] > 0:
                # NO PACE UNTIL SOMETHING HAS BEEN MEASURED, which is a real state rather than
                # an edge case: a resume can skip hundreds of groups before it reaches one that
                # needs running, and an estimate of zero would read as "nearly done".
                estimate = elapsed * left / state["measured"]
                note = " (flat)"
            else:
                estimate = None
                note = " (nothing measured yet)"

        shown = "?" if estimate is None else clock(estimate)
        print(
            f"    group {state['done']}/{len(groups)}  "
            f"measured {state['measured']}/{groups_with_work}  "
            f"elapsed {clock(elapsed)}  est. left ~{shown}{note}",
            flush=True,
        )

    # A worker's output is held and printed whole when its group finishes, so that several
    # groups at once do not interleave into something nobody can read. That is what
    # `measure_group` returning its output rather than printing it is for.
    with ThreadPoolExecutor(max_workers=workers) as pool:
        futures = {pool.submit(run.measure_group, conversation): conversation for conversation in groups}
        for future in _as_completed_in_submission_order(futures):
            conversation = futures[future]
            try:
                tally = future.result()
            except SystemExit:
                raise
            except Exception:  # pylint: disable=broad-except
                # WORKER LOST. Its finished rows are already in the TSV - they are appended as
                # they happen - so this loses only the tally, and saying so beats adding zero.
                print(f"WORKER LOST for group {conversation} - its finished rows are in the TSV, its tally is not")
                traceback.print_exc()
                continue
            reap(conversation, tally)


def _as_completed_in_submission_order(futures):
    """Futures as they finish, which is what `concurrent.futures.as_completed` gives.

    Wrapped so the import stays beside its one use and so the reason is written down: reaping
    in SUBMISSION order would make one slow group hold back every group behind it, which is
    the interleaving the buffered output exists to avoid in the first place.
    """
    from concurrent.futures import as_completed

    return as_completed(futures)


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
        help="conversation ids to measure, or `all` for every group in the game",
    )
    return parser


def measure(named):
    conversations = []
    if len(named) == 1 and named[0] == "all":
        conversations = "all"
    else:
        for value in named:
            try:
                conversations.append(int(value))
            except ValueError:
                refuse(f"{value!r} is not a conversation id")

    run = Run([] if conversations == "all" else conversations or DEFAULT_CONVERSATIONS)

    if conversations == "all":
        run.enumerate_groups()

    run.arrange_census()
    run.read_past_runs()
    run.read_folder()

    # THE EMPTY GROUPS ARE SKIPPED ENTIRELY, AND NOT WRITTEN DOWN.
    #
    # THIS REVERSES A DELIBERATE DECISION, so it is worth saying that it was one rather than an
    # oversight. The folder used to get a TSV per empty group with a NO-ROWS row per profile,
    # on the argument that "pruned is not the same as forgotten". de-cziy: the user weighed
    # that and decided it does not earn its keep here. 901 of the game's 1,422 groups reach
    # nothing, so the run wrote 901 files and 1,802 rows - over ninety-nine per cent of both -
    # to say nothing, and anything reading the folder had to filter them out first.
    #
    # WHAT MAKES IT SAFE is that the information does not live only in those rows. groups.log
    # holds one line per pruned group, in the wording the row logs used, and groups.tsv holds
    # the whole enumeration with its reachable count. KEEPING BOTH IS PART OF THIS.
    if run.empty_groups:
        run.no_row_groups = len(run.empty_groups)
        print(f"{run.no_row_groups} group(s) reach nothing from their start and are skipped without running")
        print(f"  which, and why, is in {run.logs / 'groups.log'}; the whole enumeration is in groups.tsv")

    run.total_rows = len(run.conversations) * len(run.profiles)

    # EVERY ROW THIS RUN WILL DO, in the order it will do them, so that at any point the run
    # can say which rows are still ahead of it - which is what the weighted estimate needs and
    # a count of rows cannot give.
    run.left_spec = [(str(conversation), profile) for conversation in run.conversations for profile in run.profiles]
    already = sum(1 for key in run.left_spec if run.row_done.get(key))

    print(f"{run.total_rows} rows, {run.row_seconds}s per engine per row, started {time.strftime('%H:%M:%S')}")
    # WHICH INDEX, beside the cap, because a run reported as a measurement has to say what it
    # was measured against and nothing in a TSV records it. The trimmed index is the mod's own,
    # and tests/shipped_index.rs is why rows measured against it are the same rows.
    print(f"index: conversation_index.trimmed.jsonl (the shipped index), memory {run.full_budget_mb} MB")
    if already:
        print(f"resuming in {run.logs}: {already} row(s) already measured, and they will be skipped")

    # NO MEMORY ARM HERE, and the asymmetry with the census driver is the point rather than an
    # omission. A matrix run DIVIDES one budget among its workers, so what it commits is
    # `full_budget_mb` whatever the worker count - four workers at 1,536 MB is the same six
    # gigabytes as one worker at 6,144. There is no number of workers that asks the machine
    # for more, so bounding the count by free memory would bound nothing. The census divides
    # nothing, which is why it passes a per-worker figure and this does not.
    workers = common.default_workers()
    serial_groups = env_int("SERIAL_GROUPS", 0)
    headroom = env_int("MEMORY_HEADROOM", 2)
    worker_mb = run.full_budget_mb // max(1, workers)
    worker_nodes = worker_mb * 1024 * 1024 // run.bytes_per_node
    fits_nodes = worker_nodes // max(1, headroom)

    settle = split_message(run, workers, serial_groups, fits_nodes, worker_nodes, worker_mb)
    serial_done = serial_phase(run, workers, serial_groups, fits_nodes, settle)

    remaining = run.conversations[serial_done:]
    if remaining:
        parallel_phase(run, remaining, workers, worker_mb)

    summarise(run)


def summarise(run):
    written = sorted(run.logs.glob("performance-matrix-*.tsv"))
    print()
    print(f"wrote {len(written)} file(s) in: {run.logs}")

    # WHAT THE REUSED CENSUS COST, said plainly. A repair is real time in the middle of a run,
    # so a run that repaired should say so rather than merely run long. AND MANY REPAIRS MEAN
    # THE WRONG CENSUS, not a few stale groups.
    if run.census_reused:
        if run.census_repaired:
            print(f"reused a census and REPAIRED {len(run.census_repaired)} group(s) that contradicted it:")
            print("  " + " ".join(str(g) for g in sorted(run.census_repaired)))
            print(f"  it came from {run.census_reused_from} - if this number is large, take a fresh one")
            print("  with CENSUS_REUSE=no rather than repairing group by group.")
        else:
            print(f"reused a census from {run.census_reused_from}; no group contradicted it.")

    # NAMED ONE BY ONE ONLY WHEN THERE ARE FEW. A whole-game run writes fourteen hundred of
    # them and the list is not a summary of anything.
    if len(written) <= 20:
        for path in written:
            print(path)

    if run.skipped_rows:
        print(f"{run.skipped_rows} row(s) were already in that folder and were skipped.")

    # GROUPS NOW, NOT ROWS, because rows are what stopped being written for them (de-cziy).
    if run.no_row_groups:
        print(f"{run.no_row_groups} group(s) had nothing to measure and were skipped: no group builds")
        print("from the start, it has no entry 0, or nothing is reachable from it. See groups.log.")

    # A TRIPWIRE RATHER THAN A TALLY. Since de-cziy the row loop REFUSES a group with no rows
    # instead of recording one, and a whole-game run prunes them before any process starts - so
    # this counter can no longer be incremented by any path. It is kept because if it ever does
    # fire, the two halves that ask `measurable` have disagreed.
    if run.no_rows:
        print()
        print(f"*** {run.no_rows} row(s) reported no rows without the run refusing, which should")
        print("*** not be possible: the enumeration and the row loop disagree.")

    if run.not_measured:
        print()
        print(f"*** {run.not_measured} row(s) NOT MEASURED: this machine could not supply the budget.")
        print("*** Those rows are not results. Rerun them with the memory free before reading")
        print("*** this run as a measurement. Re-running with the same MATRIX_OUT retries")
        print("*** exactly those rows and leaves everything else alone.")


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        measure(args.conversations)
    except SystemExit:
        raise
    except KeyboardInterrupt:
        # A KILL IS A NORMAL WAY TO END A RUN. Rows are appended as they finish, so everything
        # before this is in the folder and the identical command resumes into it.
        print("\nstopped; re-run the same command with the same MATRIX_OUT to resume")
        return 130
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
