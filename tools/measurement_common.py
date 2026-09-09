#!/usr/bin/env python

"""What the matrix driver and the census driver both need, kept in one place.

## Why this module exists

de-42lg. Both drivers run ONE PROCESS PER GROUP, several at a time, against a binary they
have to find and build, under a memory budget they have to divide - and the memory arithmetic
is the part that must not drift, because getting it wrong is a run that DIES rather than a run
that is wrong. Two files each with their own reap loop and their own division is two things to
keep in step.

The shell versions had exactly that problem and it was the reason de-42lg was filed against
the census rather than solved inside it: `tools/measure-matrix.sh` grew a whole parallel
implementation for de-thlz.4 and `tools/measure-census.sh` had none, so parallelising the
census meant either copying that implementation or extracting it. This is the extraction, done
once the matrix driver was Python (de-12wr.1) and the extraction was a module rather than a
sourced shell file.

## What is NOT here

Anything either driver does alone. The matrix's weighted estimate reads matrix TSVs and means
nothing to a census; the census's journals mean nothing to the matrix. Sharing those would be
sharing a coincidence.
"""

import os
import re
import subprocess
import sys

from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "measurements"

TAB = "\t"


def write_lf(path, text):
    """Write text with LF endings, whatever platform this is on.

    PYTHON'S TEXT MODE TRANSLATES `\\n` TO `os.linesep` ON WRITE, so a driver that simply
    called `write_text` produced CRLF on Windows where the shell drivers it replaced produced
    LF. That is not cosmetic: the artefacts are read back by awk and by other tools, and a
    trailing carriage return turns a numeric field into a string - `awk -F'\\t' '$4 > 0'` over
    a CRLF groups.tsv compares "0\\r" against 0 AS TEXT and passes every line, which is how a
    filter for "groups with rows" silently returned the 901 groups that have none.

    Found 2026-09-09 while timing de-12wr.6, and it had already reached a whole-game folder.
    """
    Path(path).write_text(text, encoding="utf-8", newline="\n")


def open_lf(path, mode="a"):
    """Open a file for text writing with LF endings. See `write_lf` for why."""
    return Path(path).open(mode, encoding="utf-8", newline="\n")


def refuse(message, code=2):
    """Stops the run, saying why, rather than measuring something nobody asked for."""
    print(message, file=sys.stderr)
    raise SystemExit(code)


# HOW LONG A THING HAS TO TAKE BEFORE IT IS WORTH A LINE OF ITS OWN.
#
# From the user, de-12wr.8. On the tail - which is most of a whole-game run - a row is a few
# hundred milliseconds and five of them scroll past faster than anyone reads, while the group
# line that follows says everything. A line printed for each is noise that buries the lines
# that matter.
#
# IT ALSO SETTLES THE INTERLEAVING. A row that never printed its opening cannot have the
# measurement's own progress lines land mid-line, so the one-line form is safe precisely where
# it is used; a row slow enough to earn an opening is a row slow enough to want those progress
# lines underneath it.
WORTH_A_LINE = 2.0


def progress_line(done, total, item, seconds=None, elapsed=None, estimate=None, note=""):
    """The one progress line every driver prints, in every phase.

    ## Why there is a function rather than four format strings

    de-12wr.8. The matrix's serial phase, its parallel phase and the census each grew their own
    layout, saying the same four things - where the run is, how far in, how long it has taken,
    how long is left - in three different orders with three different punctuations. A reader
    switching between a census log and a matrix log paid for that every time, and two separate
    issues had already moved one of them a step closer to the others by hand.

    ## Fixed width, which is the whole point of printing hundreds of them

    The count is padded to the width of the total and the item to the width it was given, so
    the elapsed and estimate columns stay in one place down the page instead of walking left
    and right. That is what makes two adjacent lines comparable at a glance, which is the only
    reason a per-item duration is printed at all.

    Returns the line rather than printing it, because the matrix's parallel phase buffers a
    group's output and prints it whole when the group is reaped - so that several groups at
    once do not interleave into something nobody can read.
    """
    width = len(str(total))
    percent = done * 100 // max(1, total)
    line = f"[{done:>{width}}/{total} {percent:>3}%] {item}"
    if seconds is not None:
        line += f"  {seconds:>8.2f}s"
    if elapsed is not None:
        line += f"  elapsed {clock(elapsed)}"
    if estimate is not None:
        line += f"  est. left ~{clock(estimate)}"
    if note:
        line += f"  {note}"
    return line


def clock(seconds):
    """h:mm:ss. A run of this length is watched rather than read afterwards, and seconds
    since the epoch is not something a person can watch."""
    seconds = max(0, int(seconds))
    return f"{seconds // 3600}:{(seconds % 3600) // 60:02d}:{seconds % 60:02d}"


# The prefix on every environment variable this project defines. See CLAUDE.md for the rule
# and docs/environment.md for the list.
ENV_PREFIX = "DEGCT_"


def qualified(name):
    """The full name of one of ours, from its bare one.

    Idempotent, because callers build names from both halves - a bare one they were given and
    a full one read back out of a message - and DEGCT_DEGCT_CONVERSATION would be unset,
    silently, and read as a default.
    """
    return name if name.startswith(ENV_PREFIX) else ENV_PREFIX + name


def env(name, fallback=None, foreign=False):
    """One of ours, by its BARE name: `env("MATRIX_OUT")` reads DEGCT_MATRIX_OUT.

    ## Why a helper rather than a convention

    A convention a person has to remember grows exceptions, and the whole reason the prefix
    exists is that the shell owns a pile of short generic names - GROUPS is a built-in array,
    and assigning to it looks like it works. The prefix is applied here, so a new variable is
    named right because there is no other way to name it.

    `foreign=True` reads a name somebody else owns - PATH, CARGO_TARGET_DIR,
    NUMBER_OF_PROCESSORS - under its own spelling. It is a named argument rather than a second
    function so the call site says which of the two it means.
    """
    key = name if foreign else qualified(name)
    value = os.environ.get(key)
    return fallback if value is None else value


def env_is_set(name):
    """Whether one of ours is set at all, whatever it is set to.

    The shape a flag takes here: several measurements switch on PRESENCE rather than value, so
    DEGCT_CENSUS=1 and DEGCT_CENSUS= mean the same and neither has to be parsed.
    """
    return qualified(name) in os.environ


def env_list(name):
    """A comma or space separated list from one of ours, or None where unset or empty."""
    raw = (env(name) or "").strip()
    if not raw:
        return None
    return [piece for piece in re.split(r"[,\s]+", raw) if piece]


def env_int(name, fallback):
    raw = (env(name) or "").strip()
    if not raw:
        return fallback
    try:
        return int(raw)
    except ValueError:
        refuse(f"{qualified(name)}={raw!r} is not a number")


def env_for_child(**names):
    """An environment dict for a child process, with our names qualified.

    Handed the BARE names - `env_for_child(CONVERSATION="631", NO_HEADER="1")` - so the same
    rule that governs reading governs setting, and a driver cannot pass a child a variable the
    measurement will not recognise. Everything already in os.environ is carried through
    untouched, because a child needs PATH and the rest.
    """
    child = dict(os.environ)
    for name, value in names.items():
        child[qualified(name)] = str(value)
    return child


def folders_newest_first():
    """Every run folder under measurements/logs, newest first by MODIFICATION TIME.

    By mtime rather than by name, because not every folder is date-stamped and a resumed run
    is genuinely more recent than its name says.
    """
    try:
        folders = [p for p in (OUT / "logs").iterdir() if p.is_dir()]
    except OSError:
        return []
    return sorted(folders, key=lambda p: p.stat().st_mtime, reverse=True)


def build_measurement(example, quiet=False):
    """Builds `example` once and returns its binary, or stops the run.

    BUILT ONCE, UP FRONT, and then called DIRECTLY rather than through `cargo run`. Letting
    each row build would put a compile inside the timing of whichever row happened to run
    first, and `cargo run` re-checks the build on every invocation - measured 2026-09-08 on an
    up-to-date tree at 0.552s against 0.042s for the binary, which over a whole-game run is
    about two hours spent re-answering one question. It is also a LOCK: concurrent `cargo
    run`s serialise on the target directory, which is fatal to running groups side by side.

    CHECKED ONCE, HERE. A missing binary called directly gives an error per group, and every
    one of those would be recorded as a crashed group - a build failure written into the
    folder as hundreds of findings.
    """
    if not quiet:
        print(f"building {example}...")
    subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "--example",
            example,
            "--manifest-path",
            str(ROOT / "Cargo.toml"),
        ],
        capture_output=True,
        check=False,
    )

    target = Path(env("CARGO_TARGET_DIR", foreign=True) or (ROOT / "target"))
    binary = target / "release" / "examples" / example
    if not os.access(binary, os.X_OK):
        binary = binary.with_suffix(".exe")
    if not os.access(binary, os.X_OK):
        refuse(f"the measurement did not build - no runnable binary at {binary}", code=1)
    return binary


def ask(binary, extra_env, base_env=None):
    """One question put to the measurement, answered on stdout."""
    env = dict(base_env if base_env is not None else os.environ)
    env.update(extra_env)
    return subprocess.run([str(binary)], capture_output=True, text=True, env=env, errors="replace")


def read_constants(binary, base_env=None):
    """The numbers a driver does arithmetic with, asked for rather than transcribed.

    The memory budget and the bytes a diagram node costs both live in src/symbolic/budget.rs,
    and the shell drivers kept their own copies - "the two numbers here that have to be kept in
    step with the Rust by hand", as the matrix driver put it. A hand-kept copy of a constant is
    wrong silently, and this one is wrong in the direction that manufactures rows: too large a
    worker share and the workers race for memory the machine has not got.

    AN OLDER BINARY SAYS NOTHING AND IS NOT AN ERROR. The caller falls back to the figures the
    shell carried, which is exactly where it would have been anyway.
    """
    answer = ask(binary, {qualified("CONSTANTS_ONLY"): "1"}, base_env)
    constants = {}
    for line in answer.stdout.splitlines():
        name, _, value = line.partition(TAB)
        if value.strip().isdigit():
            constants[name.strip()] = int(value.strip())
    return constants


# HOW MUCH OF THE MACHINE A RUN LEAVES ALONE.
#
# Five per cent OF TOTAL memory, held back from what is available. Of total rather than of
# available, deliberately: the reserve is for the machine to keep working in - the editor
# growing, a build starting, the page cache wanting somewhere to live - and how much room that
# needs is a property of the machine, not of how much happened to be free at the instant the
# reading was taken. Five per cent of available would hold back almost nothing exactly when
# memory is tight, which is when the reserve matters.
#
# AND THE COST OF THE TWO MISTAKES IS NOT SYMMETRIC. One worker too few is a run that takes
# slightly longer; one worker too many is a manager whose preallocation aborts, which takes
# the process down and writes a CRASHED row that reads as a finding about the search.
MEMORY_WIGGLE_ROOM = 0.05


def system_memory_mb():
    """(total, available) in megabytes, or None where the platform cannot be asked.

    BOTH, because the two are used for different things: what a run may spend is measured
    against AVAILABLE, and the reserve it leaves behind is measured against TOTAL.

    NONE RATHER THAN A GUESS where the platform does not answer. A caller that cannot learn
    this should fall back to the core count rather than to a number somebody made up, because
    a made-up figure is wrong in the direction that starts too many workers.
    """
    if sys.platform == "win32":
        import ctypes

        class _Status(ctypes.Structure):
            _fields_ = [
                ("dwLength", ctypes.c_ulong),
                ("dwMemoryLoad", ctypes.c_ulong),
                ("ullTotalPhys", ctypes.c_ulonglong),
                ("ullAvailPhys", ctypes.c_ulonglong),
                ("ullTotalPageFile", ctypes.c_ulonglong),
                ("ullAvailPageFile", ctypes.c_ulonglong),
                ("ullTotalVirtual", ctypes.c_ulonglong),
                ("ullAvailVirtual", ctypes.c_ulonglong),
                ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
            ]

        status = _Status()
        status.dwLength = ctypes.sizeof(_Status)
        if not ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
            return None
        megabyte = 1024 * 1024
        return status.ullTotalPhys // megabyte, status.ullAvailPhys // megabyte

    # MemAvailable rather than MemFree, for the reason the kernel added it: MemFree omits the
    # page cache, which is reclaimable, and reads as a machine with far less to give than it
    # has.
    try:
        total = None
        available = None
        for line in Path("/proc/meminfo").read_text(encoding="utf-8").splitlines():
            if line.startswith("MemTotal:"):
                total = int(line.split()[1]) // 1024
            elif line.startswith("MemAvailable:"):
                available = int(line.split()[1]) // 1024
        if total is not None and available is not None:
            return total, available
    except (OSError, ValueError, IndexError):
        pass

    try:
        page = os.sysconf("SC_PAGE_SIZE")
        megabyte = 1024 * 1024
        return (
            os.sysconf("SC_PHYS_PAGES") * page // megabyte,
            os.sysconf("SC_AVPHYS_PAGES") * page // megabyte,
        )
    except (AttributeError, ValueError, OSError):
        return None


def default_workers(memory_per_worker_mb=None):
    """How many groups to run at once: the cores, or what the free memory affords, whichever
    is smaller.

    THE MINIMUM OF THE TWO, not the maximum. More workers than cores buys little on a
    workload this process-bound, and more workers than the memory affords is the failure the
    matrix driver's per-worker division exists to prevent - every worker's manager
    PREALLOCATES its budget, so the memory is committed before any of them measures anything.
    Overshooting turns into NOT-MEASURED rows where a probe was refused and CRASHED rows where
    a probe passed and the allocation aborted, which is a run that manufactures findings.

    `memory_per_worker_mb` is what ONE worker commits, and the two drivers answer it
    differently: a matrix row is allowed the full measurement budget and divides it, so it
    passes its already-divided share; a census process takes a fixed
    `DiagramBudget::over_a_group()` that nothing on the command line moves, so it passes that.
    None skips the memory arm entirely.

    NOTHING HERE IS A PROPERTY OF ONE MACHINE. Both terms are read off the machine the run is
    on, which is the whole point: a constant picked from one box is silently in the wrong
    place on the next, and in the direction that matters.

    DEGCT_WORKERS=n overrides it outright, including upwards - a person who knows what their machine
    can take is not second-guessed.
    """
    named = env("WORKERS", "").strip()
    if named:
        return max(1, env_int("WORKERS", 1))

    cores = os.cpu_count() or 1
    if not memory_per_worker_mb:
        return cores

    memory = system_memory_mb()
    if memory is None:
        return cores
    total, available = memory
    # THE RESERVE COMES OFF WHAT IS AVAILABLE AND IS SIZED BY WHAT IS TOTAL. See
    # MEMORY_WIGGLE_ROOM.
    spendable = available - total * MEMORY_WIGGLE_ROOM
    return max(1, min(cores, int(spendable // memory_per_worker_mb)))


def run_groups(groups, work, workers, reap):
    """Run `work(group)` over `groups`, at most `workers` at once, `reap`ing as they finish.

    GROUPS RUN IN PARALLEL, NEVER THE THINGS INSIDE THEM, which is what keeps two workers off
    one file: a matrix group owns its performance-matrix-<start>.tsv and a census group owns
    its groups/<start>.row.tsv. The append-as-it-finishes resume then needs no locking at all.

    REAPED IN COMPLETION ORDER rather than submission order, because reaping in submission
    order would make one slow group hold back every group behind it - and the whole reason a
    worker's output is buffered and printed at reap is so that several groups at once do not
    interleave into something nobody can read.

    A WORKER THAT RAISES COSTS ITS TALLY AND NOTHING ELSE. Whatever rows it finished are
    already on disk, because both drivers write them as they happen; what is lost is the
    counting, and saying so beats adding zero. The exception is `SystemExit`, which is a
    driver refusing the run - that has to reach the top rather than be counted as one group
    going wrong.
    """
    from concurrent.futures import ThreadPoolExecutor, as_completed

    if workers <= 1:
        for group in groups:
            reap(group, work(group))
        return

    with ThreadPoolExecutor(max_workers=workers) as pool:
        futures = {pool.submit(work, group): group for group in groups}
        for future in as_completed(futures):
            group = futures[future]
            try:
                result = future.result()
            except SystemExit:
                raise
            except Exception:  # pylint: disable=broad-except
                import traceback

                print(f"WORKER LOST for group {group} - what it finished is on disk, its tally is not")
                traceback.print_exc()
                continue
            reap(group, result)
