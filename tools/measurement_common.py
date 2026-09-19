#!/usr/bin/env python

"""What the matrix driver and the census driver both need, kept in one place.

## Why this module exists

de-42lg. Both drivers run ONE PROCESS PER GROUP, several at a time, against a binary they
have to find and build, under a memory budget they have to divide - and the memory arithmetic
is the part that must not drift, because getting it wrong is a run that DIES rather than a run
that is wrong. Two files each with their own reap loop and their own division is two things to
keep in step.

The alternative is a driver that reaps its own workers and divides its own budget, and a
second driver that copies both. de-42lg was filed against the census for exactly that reason:
one driver had a parallel implementation and the other had none, so parallelising the second
meant copying the first. A module is what makes copying unnecessary.

## What is NOT here

Anything either driver does alone. The matrix's weighted estimate reads matrix TSVs and means
nothing to a census; the census's journals mean nothing to the matrix. Sharing those would be
sharing a coincidence.
"""

import csv
import hashlib
import io
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time

from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The one kind of run whose numbers a cold first run can spoil. See `measure-menus.COLD_FOLDER`.
PERFORMANCE_KIND = "performance"

# The three kinds `tools/run-logged.sh` knows, each of which is also its tree's name. EVERY RUN
# IS A MEASUREMENT - of correctness, of data, or of timing - so none of them is called "measure";
# the kind says WHAT is measured, and a kind and its tree say the same word.
KINDS = (PERFORMANCE_KIND, "testing", "analysis")

# Where a log of a given kind goes, under its tree.
LOGS = "logs"

OUT = ROOT / PERFORMANCE_KIND

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


def watchable_output():
    """Make this process's stdout line-buffered, so a long run can be watched while it runs.

    WHAT IT FIXES. `tools/run-logged.sh` pipes a driver into `tee`, so stdout is a pipe rather
    than a terminal and Python block-buffers it at 8 KB - about 270 progress lines. A run
    therefore appears to stall for minutes and then emit a wall of output, at flush boundaries
    that fall in the middle of whatever it was doing. The comment above that pipe says it is a
    tee rather than a redirect "so a long run can still be watched while it runs", which the
    buffering defeats entirely.

    PYTHON ONLY. Rust's `println!` writes through a `LineWriter` and stays line-buffered when
    piped, which is why the cargo-side measurements never showed this.

    CALLED BY THE DRIVER rather than done on import, because a module that reconfigures the
    interpreter's stdout merely by being imported is a surprise to anything that imports it
    for one function.

    GUARDED, because `sys.stdout` is only a `TextIOWrapper` when it is the real one - a test
    harness or a caller that captured output has replaced it with something that has no
    `reconfigure`, and buffering is not worth an exception in either.
    """
    if isinstance(sys.stdout, io.TextIOWrapper):
        sys.stdout.reconfigure(line_buffering=True)


def refuse(message, code=2):
    """Stops the run, saying why, rather than measuring something nobody asked for."""
    print(message, file=sys.stderr)
    raise SystemExit(code)


def progress_line(done, total, item, seconds=None, elapsed=None, estimate=None, note=""):
    """The one progress line every driver prints, in every phase.

    ## Why there is a function rather than four format strings

    de-12wr.8. A run's serial phase and its parallel phase say the same four things - where the
    run is, how far in, how long it has taken, how long is left - and one layout says them the
    same way in both, so a reader switching between two logs is not paying for three orders and
    three punctuations.

    ## Fixed width, which is the whole point of printing hundreds of them

    The count is padded to the width of the total and the item to the width it was given, so
    the elapsed and estimate columns stay in one place down the page instead of walking left
    and right. That is what makes two adjacent lines comparable at a glance, which is the only
    reason a per-item duration is printed at all.

    Returns the line rather than printing it, because a parallel phase buffers a group's output
    and prints it whole when the group is reaped - so that several groups at once do not
    interleave into something nobody can read.
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
    """One of ours, by its BARE name: `env("MENUS_OUT")` reads DEGCT_MENUS_OUT.

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
    DEGCT_NOLIMIT=1 and DEGCT_NOLIMIT= mean the same and neither has to be parsed.
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

    Handed the BARE names - `env_for_child(CONVERSATION="631", HEADER="1")` - so the same
    rule that governs reading governs setting, and a driver cannot pass a child a variable the
    measurement will not recognise. Everything already in os.environ is carried through
    untouched, because a child needs PATH and the rest.
    """
    child = dict(os.environ)
    for name, value in names.items():
        child[qualified(name)] = str(value)
    return child


def bash(hint=""):
    """A bash that shares this process's idea of what a drive letter means.

    NOT THE BARE NAME. Handing `bash` to CreateProcess searches System32 before PATH, and on
    a machine with WSL installed that is WSL's launcher - a different machine's shell. It
    runs, so nothing looks wrong, but it calls this drive `/mnt/d`: a Windows path handed to
    it comes back "No such file or directory" and a path it prints back is one this process
    cannot open.

    PATH first, then wherever git lives, since Git for Windows ships the bash that goes with
    it and this repository needs git anyway. On anything but Windows the first answer is the
    only one.
    """
    system_root = Path(os.environ.get("SystemRoot", r"C:\Windows")).resolve()

    def usable(candidate):
        if candidate is None:
            return None
        path = Path(candidate)
        if not path.is_file():
            return None
        try:
            path.resolve().relative_to(system_root)
        except ValueError:
            return str(path)
        return None

    found = usable(shutil.which("bash"))
    if found:
        return found

    git = shutil.which("git")
    if git:
        for folder in Path(git).resolve().parents:
            for relative in ("usr/bin/bash.exe", "bin/bash.exe"):
                found = usable(folder / relative)
                if found:
                    return found

    refuse("no bash to run tools/run-logged.sh with - install Git for Windows" + hint, code=1)


# What a folder name may be before it counts as a path rather than a label: one component of
# ordinary name characters. Anything holding a separator, a drive or a dot-dot is a path and is
# taken literally.
LABEL = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")


def is_label(value):
    """Whether `value` names a run rather than saying where to put it."""
    return bool(LABEL.match(str(value))) and ".." not in str(value)


# What stands between a run's own name and the label a caller gave it. TWO characters where
# one would read the same, because a single underscore already separates the name's own fields
# and a label joined by one would read as another field.
LABEL_SEPARATOR = "__"


def run_kind(asked):
    """What a run is FOR: which tree its artefacts belong in, and whether it throws a pass away.

    `asked` is the driver's own --kind, which WINS over the wrapper's DEGCT_RUN_KIND: the
    wrapper is told what a run measures before the tool is, and a performance tool pressed into
    deriving a dataset is exactly the case where the two differ.

    UNKNOWN COUNTS AS TIMING, for the reason `measure-menus.takes_cold_run` gives: an unwrapped
    invocation that says nothing about itself pays the cold run rather than risk a comparison
    taken without one.
    """
    return asked or env("RUN_KIND", PERFORMANCE_KIND)


def in_tree(folder, kind):
    """`folder` under the tree `kind` names, where it sits in one of those trees at all.

    THE KIND DECIDES THE TREE FOR THE ROWS TOO, not only for the transcript. A performance tool
    asked for a DATASET writes a dataset, and rows filed among the timings put numbers nobody
    took as measurements into the sample of everything that reads performance/logs -
    `tools/cold-run-effect.py` counts the multi-run passes there, and any later survey of what
    has been measured reads the same tree.

    THE NAME STILL PAIRS THEM when the kind puts the rows in a different tree from the
    transcript: same date folder, same name, so the transcript a folder came from is one
    directory across and readable off the name.

    ANYWHERE ELSE IS LEFT WHERE IT IS. DEGCT_RUN_LOG_DIR puts a log wherever it says, and a
    caller who named the directory has already answered the question this asks.
    """
    folder = Path(folder)
    try:
        relative = folder.resolve().relative_to(ROOT)
    except ValueError:
        return folder
    if len(relative.parts) > 2 and relative.parts[0] in KINDS and relative.parts[1] == LOGS:
        return ROOT.joinpath(kind, *relative.parts[1:])
    return folder


def folder_for(value, tool, verb, out_variable, kind):
    """Where a driver told `value` should write: a path as given, or a label's folder.

    ## Two things one variable can be

    A PATH is taken literally, which is what it always was: `DEGCT_MENUS_OUT=/tmp/rows` writes
    there and resumes there.

    A LABEL - one plain word, no separators - names the run instead of placing it, and is a
    SUFFIX on the name the run would have had anyway:

        DEGCT_MENUS_OUT=qy5t-before
        -> performance/logs/2026-09-18/2026-09-18_10,07,41_7a2d23f_measure-menus_menus__qy5t-before/

    WHY THE LABEL EXISTS. Naming the folder by hand is the common case, and a hand-named folder
    sat outside the convention every other artefact follows - so `qy5t-before/` and the
    transcript that produced it shared nothing in their names and only a file inside the folder
    said they belonged together. A label keeps the freedom to say what a run was FOR while
    letting the name say when it ran and against what.

    WHY IT IS A SUFFIX AND NOT THE WHOLE NAME. A label that replaced the name would give up the
    one thing the un-labelled case exists to provide - rows carrying their transcript's name
    exactly, so which folder belongs to which log reads off the two names without opening
    either - and it would give it up for the runs most worth pairing, since a run worth
    labelling is a run somebody meant to come back to. As a suffix both hold at once.

    ## Resuming a label

    A label reuses the MOST RECENT folder carrying it, which is how resuming a path already
    behaves: point at the same thing and it continues. A label used for the first time gets a
    new folder. The settings check still refuses a resume whose measurement differs, so reusing
    a label across a change is caught rather than silently mixed.

    EVERY TREE IS SEARCHED, not the one this run's kind names, because the kind says what a run
    measures and a resume is the same measurement continuing - a folder must not be missed, and
    a second one started beside it, over an argument about what to call the run.

    THE NAME KEEPS THE FIRST REVISION, AND THAT IS A KNOWN COST. A resumed folder is named for
    the invocation that made it, so rows added later can have been measured at another commit
    while the folder still says the first - and the code is deliberately not something a resume
    is refused over, since comparing two revisions is what a measurement is often for. Keeping
    the rows grouped is worth more than splitting them by revision, so `write_run_record` says
    so on stderr when it happens, and each invocation's own code is recorded under `resumed`.
    """
    if not is_label(value):
        return Path(value)

    existing = sorted(
        (path for tree in KINDS for path in (ROOT / tree / LOGS).glob(f"*/*{LABEL_SEPARATOR}{value}") if path.is_dir()),
        key=lambda path: path.stat().st_mtime,
    )
    if existing:
        return existing[-1]
    base = run_folder(tool, verb, out_variable, kind)
    return base.with_name(base.name + LABEL_SEPARATOR + value)


def run_folder(tool, verb, out_variable, kind):
    """One folder for this run, named the way every run log in this repository is named.

    THE SAME NAME AS THE TRANSCRIPT, EXACTLY, where there is one. A run writes two things -
    a transcript named for when it ran, and a folder of rows - and which belongs to which has
    to be readable off the two names, without opening either. So the folder is the transcript's
    path with the extension taken off:

        2026-09-18/2026-09-18_10,05,29_<revision>_measure-menus_menus.txt   the transcript
        2026-09-18/2026-09-18_10,05,29_<revision>_measure-menus_menus/      its rows

    ASKED OF THE WRAPPER RATHER THAN REBUILT, and this is why it is taken from the exported
    path rather than by asking for a fresh name: two calls to the wrapper are two readings of
    the clock, so a folder named by the second would sit a second or two after the transcript
    named by the first and the pair would no longer match.

    TWO LABELS UNDER ONE TRANSCRIPT ARE TWO FOLDERS, since the label is a suffix on this stem
    rather than a replacement for it. A whole-game pass and a subset of it run under one
    wrapped invocation are two measurements, and one folder holding both would report one set
    of numbers twice.

    ASKED OF tools/run-logged.sh RATHER THAN BUILT HERE, where there is no transcript to take
    it from. The format lives in that script and in RunLog.cs, held to each other by
    RunLogTests; a third copy in Python is one nothing holds to the other two, and it would
    drift the first time the name gains a field.

    THROUGH bash, NOT AS A PROGRAM. Windows cannot execute a shell script directly -
    CreateProcess answers WinError 193, "%1 is not a valid Win32 application" - and this is
    the one call a driver makes to the wrapper, so it stopped a run before its first row on
    the very path a person takes who sets nothing.

    IN THE TREE THE KIND NAMES, which is the same answer the wrapper gives a transcript, so the
    rows of an analysis pass are no more filed among the timings than its transcript is. See
    `in_tree`, and DEGCT_RUN_LOG_DIR still overrides both.

    THE TREE IS HANDED OVER RATHER THAN ASKED FOR BY ITS KIND, which the script would happily
    work out, because the script finds its own root with `pwd` in a bash that calls this drive
    `/c` - and `/c/Projects/...` read back by this process is a folder named `c` at the root of
    whatever drive it happens to be on. A directory this side spelt needs no converting. What
    costs is one restatement of an invariant both files already carry: a kind is its tree's
    name.

    `tool` and `verb` are what the run calls itself where the wrapper is not there to be asked:
    the driver's own name, and what it produced. `out_variable` is the driver's OUT name, said
    in the refusal when there is no bash to ask - naming the folder is how a run gets one
    without the wrapper.
    """
    transcript = env("RUN_LOG")
    if transcript:
        return in_tree(Path(transcript).with_suffix(""), kind)

    folder = subprocess.run(
        [
            bash(f", or name the run's folder with {qualified(out_variable)}"),
            str(ROOT / "tools" / "run-logged.sh"),
            "--folder-only",
            tool,
            verb,
        ],
        capture_output=True,
        text=True,
        env=env_for_child(RUN_LOG_DIR=env("RUN_LOG_DIR") or ROOT / kind / LOGS),
        check=True,
    ).stdout.strip()
    return Path(folder)


# What a run writes about itself into its folder, beside its rows.
RUN_RECORD = "run.json"

# What several runs of the menu measurement are combined into, beside their folders.
COMBINED = "combined.tsv"
SUMMARY = "summary.txt"


def read_run_record(folder):
    """A folder's run record, or None where it has none."""
    path = Path(folder) / RUN_RECORD
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def add_build_record(folder, did):
    """Adds what one build did to `folder`'s run record, under `builds`.

    APPENDED RATHER THAN WRITTEN WITH THE REST, because the record is written before anything
    is built - it is what a resume is checked against. A pass builds ONCE, so `builds` gains one
    entry saying how long cargo took and whether it actually recompiled; it grows past one only
    where a folder was resumed, and then each entry is a separate invocation's build.

    WHAT IT ANSWERS LATER: whether two runs being compared were built the same way. A revision
    and a dirty flag say what the SOURCE was; they do not say whether the binary was freshly
    linked for one of them and months old for the other, which is the difference a first-run
    timing question turns on.

    Silent where the folder has no record, because a driver may build before it writes one.
    """
    path = Path(folder) / RUN_RECORD
    if not path.exists():
        return
    record = json.loads(path.read_text(encoding="utf-8"))
    record.setdefault("builds", []).append(dict(did))
    write_lf(path, json.dumps(record, indent=2))


def read_combined(folder):
    """A several-run folder's combined rows, by conversation, each keyed by the file's own header.

    COLUMNS ARE MATCHED BY NAME, because the matrix's columns have moved, and reading them by position
    across two runs compares unrelated numbers.
    """
    path = Path(folder) / COMBINED
    with path.open(newline="") as handle:
        return {row["conv"]: row for row in csv.DictReader(handle, delimiter=TAB)}


def median_ms(row):
    """A combined row's median menu_ms, or None where the group did not measure.

    A group that did not measure carries the driver's word for why in place of a number - NO-MENU,
    CRASHED, NOT-MEASURED - and is compared by that word rather than by a cost.
    """
    try:
        return float(row["menu_ms_median"])
    except ValueError:
        return None


def git(*arguments, environment=None):
    """One git command run against this repository, its output as text."""
    return subprocess.run(
        ["git", "-C", str(ROOT), *arguments],
        capture_output=True,
        text=True,
        check=True,
        env=environment,
    ).stdout


def git_state():
    """The code a run was taken against: the revision, and for a dirty tree what it holds.

    A REVISION ALONE DOES NOT NAME THE CODE when the tree has changes, and most measurements are
    taken mid-change. So a dirty tree also records `tree`, the hash `git write-tree` gives the
    whole working tree - untracked files git does not ignore included - and `changed`, each file
    that differs with its two-letter `git status` code. Two runs with the same tree hash ran the
    same code whatever their revisions say.

    THE TREE IS HASHED THROUGH A COPY OF THE INDEX, so the index a person is staging a commit in
    is never touched by a run reading it.

    Where git cannot answer - no git, not a checkout - the revision is None and nothing else is
    claimed.
    """
    try:
        revision = git("rev-parse", "HEAD").strip()
        status = git("status", "--porcelain=v1", "--untracked-files=all")
    except (OSError, subprocess.CalledProcessError):
        return {"revision": None}

    changed = [{"status": line[:2], "path": line[3:]} for line in status.splitlines() if line.strip()]
    state = {"revision": revision, "dirty": bool(changed)}
    if not changed:
        return state

    index = Path(git("rev-parse", "--git-path", "index").strip())
    if not index.is_absolute():
        index = ROOT / index
    with tempfile.TemporaryDirectory() as scratch:
        copy = Path(scratch) / "index"
        if index.exists():
            shutil.copyfile(index, copy)
        environment = dict(os.environ, GIT_INDEX_FILE=str(copy))
        git("add", "--all", environment=environment)
        state["tree"] = git("write-tree", environment=environment).strip()
    state["changed"] = changed
    return state


def write_run_record(folder, parallelism, **details):
    """Writes what a run is into its folder as run.json, so its rows can be read later for what they are.

    WHAT IS RECORDED: when it started, the full command line and working directory, the code it
    ran - see `git_state` - every DEGCT_ variable as it was set, how many groups were measured at a
    time, whatever the driver adds in `details`, and the machine - its hostname, Python, platform,
    cores, running processes, per-processor busy percentages, and total and available memory. A
    row's milliseconds are a reading of all of that, and a folder that does not say is one whose
    numbers cannot be compared with anything.

    A RESUME MUST MATCH the settings, the algorithm and the machine, and each that does not is
    refused by name: a folder resumed under any other would be half one measurement and half
    another. Available memory, running processes and processor usage are not held to it - they
    move between two sittings on the same machine. A resume that matches keeps the first record and
    adds its own start, command line, code and environment under `resumed`, so the folder still says
    every way it was written to.
    """
    invocation = {
        "started": datetime.now().astimezone().isoformat(timespec="seconds"),
        "command": [sys.executable, *sys.argv],
        "cwd": os.getcwd(),
        "code": git_state(),
        "environment": {name: os.environ[name] for name in sorted(os.environ) if name.startswith(ENV_PREFIX)},
    }

    memory = system_memory_mb()
    record = {
        **invocation,
        "parallelism": parallelism,
        "details": details,
        "machine": {
            "hostname": platform.node(),
            "python": platform.python_version(),
            "platform": platform.platform(),
            "cpus": os.cpu_count(),
            "processes": running_processes(),
            "cpu_percent": cpu_busy_percent(),
            "memory_mb": None if memory is None else {"total": memory[0], "available": memory[1]},
        },
    }

    path = Path(folder) / RUN_RECORD
    if path.exists():
        was = json.loads(path.read_text(encoding="utf-8"))
        # AVAILABLE MEMORY IS NOT HELD TO A RESUME: it moves between two sittings on the same machine,
        # and a resume is one run finishing rather than a second run being compared with the first.
        refusals = [
            f"REFUSED ({kind}): these {kind} fields differ from the folder's record - {'; '.join(lines)}"
            for kind, lines in (
                ("settings", setting_differences(was, record)),
                ("algorithm", algorithm_differences(was, record)),
                ("hardware", hardware_differences(was, record, available_tolerance=None)),
            )
            if lines
        ]
        if refusals:
            refuse(
                f"{folder} cannot be resumed, since it would hold two measurements:\n"
                + "\n".join(refusals)
                + "\nMeasure into a new folder, or match them."
            )

        # A DIFFERENT COMMIT IS ALLOWED AND SAID OUT LOUD. The code is deliberately not part of
        # what a resume is held to - comparing two revisions is what a measurement is often for
        # - but a resumed folder keeps the FIRST invocation's revision in its name and in its
        # record, so rows added later can have been taken at another one and the name will not
        # say. Grouping the rows together is worth more than splitting them, so this warns
        # rather than refusing; `resumed` carries each invocation's own code beside its start.
        was_code = was.get("code")
        now_code = record.get("code")
        before = was_code.get("revision") if isinstance(was_code, dict) else None
        now = now_code.get("revision") if isinstance(now_code, dict) else None
        if isinstance(before, str) and isinstance(now, str) and before != now:
            print(
                f"note: {folder} was started at {before[:7]} and is being resumed at "
                f"{now[:7]}, so its rows are not all from one revision - the folder's name "
                "keeps the first. Each invocation's own code is under `resumed` in its "
                f"{RUN_RECORD}.",
                file=sys.stderr,
            )

        was.setdefault("resumed", []).append(invocation)
        record = was

    path.parent.mkdir(parents=True, exist_ok=True)
    write_lf(path, json.dumps(record, indent=2) + "\n")


# DEGCT_ variables KNOWN to change what a menu measurement measures, and so compared between runs:
# the manager's memory, the limits being off, and how many starts a menu has and how many entries
# are hunted. Every DEGCT_ variable is RECORDED whatever it is; only these decide whether two runs'
# settings are mixed, so a stray variable nothing reads cannot refuse a comparison.
#
# THE SETTLE RULE IS NOT HERE, though it does change what a run measures. It is compared through
# `parallelism`, which carries the rule as the sentence a reader sees - and that field is written
# the same way whether the values arrived as arguments or, in the folders already on disk, as
# variables. Comparing the variables as well would refuse a folder against its own equal, on the
# strength of how the number reached the run rather than what the number was.
COMPARED_VARIABLES = frozenset(
    qualified(name)
    for name in (
        "BUDGET_MB",
        # WHETHER PREPARATION WAS KEPT OR DERIVED AGAIN, which is most of what a row's
        # index_ms, graph_ms and prep_ms say. The menu columns are the same either way, and
        # that is exactly why these must not be mixed silently: half a folder taken with a
        # cache and half without reads as a measurement of nothing.
        "NO_CACHE",
        "CACHE_VERIFY",
        "NOLIMIT",
        "STARTS",
        "WALKED_PROFILE",
        "UNSEEN",
    )
)

# DEGCT_ variables that choose WHICH algorithm a run measures. Comparing two algorithms is often the
# very point of a comparison, so a difference in these is reported rather than refused - see
# `algorithm_differences`. A resume still refuses one: a folder must not hold two algorithms' rows.
ALGORITHM_VARIABLES = frozenset({qualified("MARKING")})

# Driver details a comparison does not hold two folders to.
#
# `groups` says WHICH groups were measured rather than how, and a comparison takes the groups both
# folders measured, so a different selection is not a different measurement.
#
# `kind` and `cold` say what the run was FOR and whether it threw a first pass away. Neither
# touches a counted row: the discarded pass is discarded, and what is left was measured the same
# way either side. They are recorded so a folder says whether it took one, not so that a folder
# taken before this existed refuses every folder taken after it.
UNCOMPARED_DETAILS = frozenset({"groups", "kind", "cold"})


def settings_of(record):
    """The part of a run record that changes what its rows measure.

    PARALLELISM, which puts a flat cost on groups measured side by side; the DEGCT_ variables in
    COMPARED_VARIABLES, as they were set or not; and the driver's details but the ones in
    UNCOMPARED_DETAILS. NOT the code or the algorithm, because comparing two of either is what a
    comparison is often for - `algorithm_differences` reports the second; NOT the machine, which
    `hardware_of` answers separately because it cannot be matched by re-running; and not the other
    recorded variables, which nothing is known to read.

    A record written before this existed has none of it, and reads as settings that are all None.
    """
    environment = record.get("environment") or {}
    details = record.get("details") or {}
    return {
        "parallelism": record.get("parallelism"),
        "environment": {name: environment.get(name) for name in sorted(COMPARED_VARIABLES)},
        "details": {name: value for name, value in details.items() if name not in UNCOMPARED_DETAILS},
    }


# How far apart two runs' available memory at the start may be, as a share of the larger, before
# they count as measured on different hardware. Generous on purpose: available memory moves with
# whatever else the machine is doing, and it matters to a run only where it is far short of what
# the managers commit.
AVAILABLE_MEMORY_TOLERANCE = 0.25


def running_processes():
    """How many processes Windows was running, as its process list reports it.

    RECORDED AND NEVER COMPARED: it moves from one minute to the next with whatever else is open,
    so holding two runs to it would refuse nearly every comparison. It is kept for a reader asking
    afterwards why one run was noisier than another.

    Raises where Windows refuses the list, rather than recording a count that is not its.
    """
    import ctypes

    from ctypes import wintypes

    size = 4096
    while True:
        ids = (wintypes.DWORD * size)()
        returned = wintypes.DWORD()
        if not ctypes.windll.psapi.EnumProcesses(ids, ctypes.sizeof(ids), ctypes.byref(returned)):
            raise ctypes.WinError()
        count = returned.value // ctypes.sizeof(wintypes.DWORD)
        # A full buffer may have cut the list short, so it is asked again with more room.
        if count < size:
            return count
        size *= 2


# The Windows performance counter a run's processor usage is read from: one instance per logical
# processor, and _Total.
PROCESSOR_COUNTER = r"\Processor(*)\% Processor Time"


def cpu_busy_percent():
    """How busy each processor was as a run started, and all of them together, as Windows reports it.

    FROM THE SYSTEM'S OWN PERFORMANCE COUNTER, through typeperf, which ships with Windows: one sample
    of PROCESSOR_COUNTER, rather than a figure worked out here. PER PROCESSOR because an average
    hides the case that matters to a measurement run one group at a time - one core pinned by
    something else and the rest idle reads as a quiet machine overall.

    RECORDED AND NEVER COMPARED, like `running_processes`: it is a reading of the moment, and a run
    started a minute later reads differently on an unchanged machine. It is kept for a reader asking
    afterwards why one run was noisier than another.

    Raises where typeperf fails or answers in a shape this does not read, rather than recording a
    figure that is not Windows'.
    """
    output = subprocess.run(
        ["typeperf", PROCESSOR_COUNTER, "-sc", "1"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    header, values = list(csv.reader(line for line in output.splitlines() if line.startswith('"')))[:2]

    total = None
    per_cpu = []
    for name, value in zip(header[1:], values[1:]):
        found = re.search(r"\\Processor\(([^)]*)\)", name)
        if found is None:
            raise ValueError(f"typeperf answered with a column this does not read: {name!r}")
        instance = found.group(1)
        percent = round(float(value), 1)
        if instance == "_Total":
            total = percent
        else:
            per_cpu.append((int(instance), percent))

    return {"total": total, "per_cpu": [percent for _, percent in sorted(per_cpu)]}


def hardware_of(record):
    """The machine a run record was taken on, as far as it is compared: name, cores and memory."""
    machine = record.get("machine") or {}
    memory = machine.get("memory_mb") or {}
    return {
        "hostname": machine.get("hostname"),
        "cpus": machine.get("cpus"),
        "memory_total_mb": memory.get("total"),
        "memory_available_mb": memory.get("available"),
    }


def hardware_differences(first, second, available_tolerance=AVAILABLE_MEMORY_TOLERANCE):
    """What two run records' machines disagree about, one readable line each; empty where they agree.

    The hostname, cores and total memory must match exactly. Available memory at the start must be
    within `available_tolerance` of the larger reading; None leaves it out, which is what a resume
    asks for - the same machine a sitting later.
    """
    a, b = hardware_of(first), hardware_of(second)
    differ = [
        f"{name}: {a[name]!r} -> {b[name]!r}" for name in ("hostname", "cpus", "memory_total_mb") if a[name] != b[name]
    ]
    if available_tolerance is not None:
        before, after = a["memory_available_mb"], b["memory_available_mb"]
        if before is None or after is None:
            if before != after:
                differ.append(f"memory_available_mb: {before!r} -> {after!r}")
        elif abs(before - after) > available_tolerance * max(before, after):
            differ.append(f"memory_available_mb: {before} -> {after}, more than {available_tolerance:.0%} apart")
    return differ


def algorithm_differences(first, second):
    """Which algorithm variables two run records disagree about, one `field: before -> after` line each.

    An unset variable is the driver's default algorithm, and is shown as None.
    """
    a = (first.get("environment") or {}) if first else {}
    b = (second.get("environment") or {}) if second else {}
    return [
        f"{name}: {a.get(name)!r} -> {b.get(name)!r}"
        for name in sorted(ALGORITHM_VARIABLES)
        if a.get(name) != b.get(name)
    ]


def setting_differences(first, second):
    """What two run records' settings disagree about, one `field: before -> after` line each; empty
    where they agree."""
    a, b = settings_of(first), settings_of(second)
    differ = []
    for name in a:
        if name in ("environment", "details"):
            for key in sorted(set(a[name]) | set(b[name])):
                if a[name].get(key) != b[name].get(key):
                    differ.append(f"{key}: {a[name].get(key)!r} -> {b[name].get(key)!r}")
        elif a[name] != b[name]:
            differ.append(f"{name}: {json.dumps(a[name])} -> {json.dumps(b[name])}")
    return differ


# Where a build's own output is kept, under the run folder it built for.
BUILD_LOG = "build.log"


def build_measurement(example, folder=None, quiet=False):
    """Builds `example` once and returns `(binary, what_the_build_did)`, or stops the run.

    BUILT ONCE PER PASS, UP FRONT, and then called DIRECTLY rather than through `cargo run`.

    ONCE PER PASS RATHER THAN ONCE PER RUN, and the reason is correctness before cost. A
    staleness check between two runs turns into a real compile if anything touched the tree
    meanwhile, so runs 1-2 and run 3 would measure different binaries while run.json recorded
    one revision for all three - a pass that silently measured two builds, looking exactly like
    a pass that measured one. Building before the first run makes them the same binary by
    construction rather than by nobody having edited anything.

    ONCE PER PASS RATHER THAN ONCE PER ROW for cost as well: a compile inside the timing of
    whichever row ran first, and `cargo run` re-checking the build on every invocation -
    measured 2026-09-08 on an up-to-date tree at 0.552s against 0.042s for the binary, about
    two hours over a whole-game run spent re-answering one question. It is also a LOCK:
    concurrent `cargo run`s serialise on the target directory, which is fatal to running groups
    side by side.

    ## What the build has to say for itself, and why

    A NON-ZERO EXIT IS FATAL HERE, and this is the hole it closes. Testing only that a runnable
    binary exists passes when a compile FAILED and the previous one is still sitting in the
    target directory - so the run measures old code while its record names the current
    revision. That is not a confusing error, it is a wrong answer that looks right. Cargo's own
    stderr goes out with the refusal, because "no runnable binary at <path>" names a path where
    the compiler had already said what was wrong.

    ITS OUTPUT IS KEPT whether or not it failed. A build that succeeded WITH WARNINGS is
    exactly the state where something later looks wrong for no visible reason, and capturing
    the output into a value nobody reads is how that evidence was being destroyed.

    HOW LONG IT TOOK, AND WHETHER IT DID ANYTHING, are reported because a measurement cannot
    otherwise tell a compile from a staleness check - about half a second on an unchanged tree
    against tens of seconds for real work. That distinction decided a real question and could
    not be answered from any log: whether a first run is slower because it ran a freshly linked
    binary. The binary's mtime moving is the signal, so it is taken before and after.
    """
    target = Path(env("CARGO_TARGET_DIR", foreign=True) or (ROOT / "target"))
    binary = target / "release" / "examples" / example
    if not os.access(binary, os.X_OK):
        binary = binary.with_suffix(".exe")
    before = binary.stat().st_mtime if binary.exists() else None

    if not quiet:
        print(f"building {example}...")
    began = time.monotonic()
    built = subprocess.run(
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
        text=True,
        check=False,
    )
    seconds = time.monotonic() - began

    if folder is not None:
        path = Path(folder)
        path.mkdir(parents=True, exist_ok=True)
        write_lf(
            path / BUILD_LOG,
            f"# cargo build --release --example {example}\n"
            f"# exit {built.returncode} in {seconds:.2f}s\n\n"
            f"{built.stdout}{built.stderr}",
        )

    if built.returncode != 0:
        refuse(
            f"the measurement did not build - cargo exited {built.returncode}:\n{built.stderr.rstrip()}",
            code=1,
        )

    after = binary.stat().st_mtime if binary.exists() else None
    recompiled = before != after
    if not quiet:
        did = "recompiled" if recompiled else "already current"
        print(f"  built {example} in {seconds:.2f}s ({did})")

    if not os.access(binary, os.X_OK):
        refuse(f"the measurement did not build - no runnable binary at {binary}", code=1)
    return binary, {
        "example": example,
        "seconds": round(seconds, 3),
        "status": built.returncode,
        "recompiled": recompiled,
        "sha256": binary_digest(binary),
    }


# How much of a binary is read at a time when hashing it. Large enough that a release binary
# takes a handful of reads, small enough not to hold it all in memory at once.
DIGEST_BLOCK = 1 << 20


def binary_digest(path):
    """The SHA-256 of a built binary, so a pass can PROVE every run used the same one.

    Building once per pass makes the runs agree by construction only as far as this process is
    concerned. Nothing stops another one - a cargo build in a second window, an editor's
    save-and-build, a concurrent driver - from relinking the file underneath a pass that is
    halfway through it. The mtime would move and nothing here would look; a digest taken after
    the build and checked before each run turns that into a refusal instead of a quiet
    half-and-half comparison.
    """
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(DIGEST_BLOCK), b""):
            digest.update(block)
    return digest.hexdigest()


def ask(binary, extra_env, base_env=None):
    """One question put to the measurement, answered on stdout."""
    env = dict(base_env if base_env is not None else os.environ)
    env.update(extra_env)
    return subprocess.run([str(binary)], capture_output=True, text=True, env=env, errors="replace")


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

    WHAT THIS ANSWERS IS THE DEFAULT, not the choice. A driver that lets a person name a worker
    count takes that count on its command line and only asks this when nothing was named - and it
    honours what was named, including upwards, because somebody who knows what their machine can
    take is not second-guessed.
    """
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


# How many settled groups in a row end a serial phase, and how far above the cheapest group so
# far a group may cost and still count as settled. See `Settling`.
SETTLE_GROUPS = 10
SETTLE_FACTOR = 2


class Settling:
    """Watches a heaviest-first run for the point where its cost has bottomed out.

    Measure the heavy groups one at a time, on an uncontended machine, and hand the rest to
    parallel workers only once the cost has flattened. It is a rule rather than a number of
    groups because the cost curve is not monotone - a cheap stretch can be followed by a heavy
    group - which is also why it wants a run of settled groups rather than one.

    A group counts as SETTLED when its cost is within `factor` of the cheapest group so far OR at
    most `ms` outright, and it `fits` - a driver that checks what a worker can hold says so,
    one that does not passes True. `groups` settled groups in a row end the watch; any other
    group resets the count. So at least `groups` groups are always measured one at a time.

    The absolute arm is the driver's to choose, because what "small" means depends on what one
    group costs: a matrix group is many rows and bottoms out near two seconds, a menu near
    twenty milliseconds.
    """

    def __init__(self, groups, factor, ms):
        self.groups = groups
        self.factor = factor
        self.ms = ms
        self.floor_ms = None
        self.settled = 0
        self.window_max_ms = 0
        self.window_max_nodes = 0

    @classmethod
    def add_arguments(cls, parser, ms_fallback):
        """Puts `--settle-groups`, `--settle-factor` and `--settle-ms` on a driver's parser.

        HERE RATHER THAN IN EACH DRIVER so that the three names, their help and their defaults are
        written once: a driver that spelled them itself would be a second place for the default to
        drift from. Only the absolute arm is the driver's to choose - what "small" means depends on
        what one group costs - so only that one is a parameter.
        """
        parser.add_argument(
            "--settle-groups",
            type=int,
            default=SETTLE_GROUPS,
            help="how many settled groups in a row end the one-at-a-time phase (default: %(default)s)",
        )
        parser.add_argument(
            "--settle-factor",
            type=int,
            default=SETTLE_FACTOR,
            help="a group counts as settled within this many times the cheapest group so far (default: %(default)s)",
        )
        parser.add_argument(
            "--settle-ms",
            type=int,
            default=ms_fallback,
            help=(
                "a group this cheap counts as settled outright, whatever the cheapest so far is (default: %(default)s)"
            ),
        )

    @classmethod
    def of(cls, args):
        """The rule the arguments `add_arguments` put on the parser ask for."""
        return cls(args.settle_groups, args.settle_factor, args.settle_ms)

    def reset(self):
        """A group that is not evidence the cost has bottomed out: start counting again."""
        self.settled = 0
        self.window_max_ms = 0
        self.window_max_nodes = 0

    def observe(self, group_ms, group_nodes=0, fits=True):
        """Folds in one measured group; returns whether the run has now bottomed out."""
        if self.floor_ms is None or group_ms < self.floor_ms:
            self.floor_ms = group_ms

        cheap = group_ms <= self.floor_ms * self.factor
        if self.ms > 0 and group_ms <= self.ms:
            cheap = True

        if cheap and fits:
            self.settled += 1
            self.window_max_ms = max(self.window_max_ms, group_ms)
            self.window_max_nodes = max(self.window_max_nodes, group_nodes)
        else:
            self.reset()
        return self.settled >= self.groups

    def rule(self):
        """What the watch is waiting for, in words."""
        says = f"{self.groups} in a row within {self.factor}x the cheapest group so far"
        if self.ms > 0:
            says += f" or under {self.ms}ms"
        return says
