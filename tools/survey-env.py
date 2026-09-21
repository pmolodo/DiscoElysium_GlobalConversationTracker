#!/usr/bin/env python
# run-log-kind: analysis

"""Where every DEGCT_ variable is READ, by which piece of the project, and how far from a main().

## Why this is not docs/environment.md

The table there names each variable and every tracked file that MENTIONS it - a read, a write, a
line of prose in a design note, all the same row. That is the right list for "what exists" and
the wrong one for "what would it take to move this onto a CLI argument", which needs to know
which mentions are reads, who owns the file doing the reading, and whether anything sets the name
at all. `DEGCT_MARKING` is the example to keep in mind: the table rows `src/bridge.rs` for it, and
what is in that file is a sentence about it in a comment.

READS ARE RECOGNISED THE WAY `tests/environment_table.rs` RECOGNISES THEM, from the same two
lists of call shapes, so this and the test agree about what counts as asking for a variable.
Where they would disagree, the test is right and this is the bug.

## What it does not count

A file ABOUT the variables is skipped entirely: the doc, the test that guards it, this tool, and
the three DOORS - `src/core/env.rs`, `tools/degct-env.sh`, `tools/DegctEnv.psm1`. A door takes a
name as a parameter and reads nothing of its own, so every name in one is an example.
`tools/measurement_common.py` is NOT skipped: it defines the Python door and
also reads variables of its own, and those reads are real.

A call that WRITES - `env::pass`, `env_for_child`, `degct_env_set` - is printed apart from the
reads. A variable that is only ever written is one this project hands DOWN rather than one it is
given, which is the distinction that decides whether an argument could carry it.

Usage:
    tools/survey-env.py                 # every variable, grouped by name
    tools/survey-env.py --owner engine  # only the ones some engine file reads
    tools/survey-env.py --by-owner      # one line per piece, for sizing the work
"""

import argparse
import collections
import re
import subprocess
import sys
import traceback

from pathlib import Path

###############################################################################
# Core functions
###############################################################################

PREFIX = "DEGCT_"

# Calls that take a bare name as a QUOTED first argument. The same list as
# `tests/environment_table.rs`, kept in the order that file has it.
QUOTED = [
    "env::var(",
    "env::is_set(",
    "env::number(",
    "env::pass(",
    "env::qualified(",
    "from_env(",
    "from_env_i32(",
    "numbers(",
    "env(",
    "env_is_set(",
    "env_int(",
    "env_list(",
    "qualified(",
    "env_for_child(",
]

# Calls that take a bare name UNQUOTED: bash and PowerShell.
BARE = [
    "degct_env_is_set ",
    "degct_env_set ",
    "degct_env ",
    "Get-DegctEnv ",
    "Test-DegctEnv ",
    "Set-DegctEnv ",
]

# Calls that hand a value DOWN rather than read one.
WRITERS = frozenset({"env::pass(", "env_for_child(", "degct_env_set ", "Set-DegctEnv "})

# The files ABOUT the variables, skipped entirely. The same set `tests/environment_table.rs`
# skips, and for the same reason: a file whose subject is the list cannot be evidence for it.
ABOUT = frozenset(
    {
        "docs/environment.md",
        "tests/environment_table.rs",
        "tools/survey-env.py",
        "src/core/env.rs",
        "tools/degct-env.sh",
        "tools/DegctEnv.psm1",
    }
)

# Which piece of the project a path belongs to. FIRST MATCH WINS, so the order is the rule: the
# plugin lives under `src/` too, and has to be recognised before the engine claims it.
OWNERS = [
    ("plugin", ("src/GlobalConversationTracker",)),
    ("engine", ("src/", "build.rs")),
    ("drivers", ("performance/",)),
    ("tests", ("tests/",)),
    (
        "csharp-tools",
        (
            "tools/GameAutomation",
            "tools/GameHarness",
            "tools/DialogueAsset",
            "tools/DialogueExtract",
            "tools/GlobalStateBenchmark",
        ),
    ),
    ("shell", (".sh", ".psm1", ".ps1")),
    ("python", (".py",)),
    ("docs", (".md",)),
]

SUFFIXES = (".rs", ".py", ".sh", ".psm1", ".ps1", ".cs", ".md")

# What a variable with no recognised read is labelled, which is NOT the same as unused.
#
# THREE SHAPES ESCAPE RECOGNITION, and all three are worth knowing about rather than papering
# over, because each is a different answer to "what would it take to move this to an argument":
#
#   A NAME HELD IN A CONSTANT. `tests/common/mod.rs` asks `env::is_set(ALL_COMMITTED_SAVES)`
#   where the const holds the bare name. Nothing here or in `tests/environment_table.rs` reads
#   a const, so both find that one through the prose beside it and would lose it if the comment
#   went.
#
#   A RAW EXPANSION. `tools/measure-symbolic.sh` reads `${DEGCT_MEASUREMENT}` and
#   `${DEGCT_RUN_NAME}` directly rather than through `degct_env`, which is the helper the
#   project mandates being bypassed.
#
#   A PHANTOM. `DEGCT_PROFILE` appears only in a doc comment in `crates/gct-measure/src/seen_profile.rs`.
#   Nothing sets it and nothing reads it, and it had a row in `docs/environment.md` anyway,
#   because a prose mention is what puts a row there.
NO_READ = "no recognised read"

# What a name is labelled when every file naming it also ASSIGNS it: a script's own working
# variable rather than an option anybody can set.
#
# THE PREFIX IS RIGHT ON THESE and they are not a problem to fix - the DEGCT_ rule covers a
# shell script's locals deliberately, since the collision that prompted it was a local. They are
# labelled rather than dropped because this tool's question is "where is every name touched",
# which a local answers; `docs/environment.md` asks "what can be set", which a local does not, so
# `tests/environment_table.rs` leaves them out of the table entirely.
A_LOCAL = "a script's own"


def owner_of(path):
    """Which piece owns `path`, by prefix for a directory and by suffix for a language."""
    for name, marks in OWNERS:
        for mark in marks:
            if mark.startswith(".") and path.endswith(mark):
                return name
            if not mark.startswith(".") and path.startswith(mark):
                return name
    return "other"


def tracked_files(root):
    """What git tracks, so build output and a downloaded game's files are never in it."""
    listed = subprocess.run(["git", "ls-files"], cwd=root, capture_output=True, text=True, check=True)
    return [line for line in listed.stdout.splitlines() if line.endswith(SUFFIXES)]


def asks_in(text):
    """Every (name, call, line) the text asks for, by bare name through a helper or spelled out."""
    found = []
    for number, line in enumerate(text.splitlines(), start=1):
        for call in QUOTED:
            for match in re.finditer(re.escape(call) + r'"([A-Za-z0-9_]+)"', line):
                # `std::env::var` reads somebody else's name under its own spelling.
                if call == "env::var(" and "std::env::var(" in line:
                    continue
                # PYTHON'S FOREIGN DOOR is a keyword on the same call rather than a name of its
                # own, so the argument list is what says which of the two a line means.
                if "foreign=True" in line[match.start() : line.find(")", match.end())]:
                    continue
                # A METHOD CALL IS NOT OUR DOOR. `command.env("GIT_INDEX_FILE", ..)` sets a
                # variable GIT owns, and reading it as `env(` invents a DEGCT_GIT_INDEX_FILE
                # nothing has ever defined. `tests/environment_table.rs` skips it the same way.
                if match.start() > 0 and line[match.start() - 1] == ".":
                    continue
                found.append((match.group(1).removeprefix(PREFIX), call, number))
        # `env_for_child(X=.., Y=..)` NAMES ITS VARIABLES AS KEYWORDS rather than as a quoted
        # first argument, so the shape above cannot see them. `tests/environment_table.rs` has
        # the same special case, and a difference here is a difference between the two.
        for match in re.finditer(r"env_for_child\(([^)]*)\)", line):
            for keyword in re.finditer(r"([A-Za-z_][A-Za-z0-9_]*)\s*=", match.group(1)):
                found.append((keyword.group(1).removeprefix(PREFIX), "env_for_child(", number))

        for call in BARE:
            for match in re.finditer(re.escape(call) + r"([A-Za-z0-9_]+)", line):
                found.append((match.group(1).removeprefix(PREFIX), call, number))
        for match in re.finditer(PREFIX + r"([A-Z0-9_]+)", line):
            # NOT PART OF A LONGER WORD, which is how the scratch prefix `DEGCTT_` and the
            # doubled `DEGCT_DEGCT_` stay out - the same guard `tests/environment_table.rs` has.
            before = line[match.start() - 1] if match.start() > 0 else ""
            if before.isalnum() or before == "_":
                continue
            # A DOUBLED PREFIX is a name the docs spell out to WARN about rather than a variable:
            # callers build names from both halves, and `DEGCT_DEGCT_CONVERSATION` is what that
            # goes wrong as. Stripping one off would report the inner name as real.
            if match.group(1).startswith(PREFIX):
                continue
            found.append((match.group(1), "spelled-out", number))
    return found


def assignments_in(text):
    """Every bare name this text ASSIGNS, as a shell script assigns one: `DEGCT_NAME=`.

    The same rule `tests/environment_table.rs` uses to tell a script's own variable from an
    option, and a difference here is a difference between the two.
    """
    return {match.group(1) for match in re.finditer(PREFIX + r"([A-Z0-9_]+)\+?=(?!=)", text)}


def sites(root):
    """Every asking site in the repository, keyed on the bare variable name."""
    where = collections.defaultdict(list)
    for path in tracked_files(root):
        # THE FILES ABOUT THE VARIABLES, skipped entirely rather than only for their reads: the
        # doc and the test that guards it name every variable by construction, and a door names
        # one to show how it is opened. The same set `tests/environment_table.rs` skips, because
        # this and that are supposed to agree.
        if path in ABOUT:
            continue
        text = (root / path).read_text(encoding="utf-8", errors="replace")
        assigned = assignments_in(text)
        for name, call, number in asks_in(text):
            where[name].append((path, call, number, name in assigned))
    return where


def split(found):
    """One variable's sites as (reads, writes), prose left out of both."""
    reads = [site for site in found if site[1] != "spelled-out" and site[1] not in WRITERS]
    writes = [site for site in found if site[1] in WRITERS]
    return reads, writes


def is_a_script_local(found):
    """Whether every file naming this one also assigns it. See `A_LOCAL`."""
    return bool(found) and all(site[3] for site in found)


def survey(root, only_owner, by_owner):
    root = Path(root)
    where = sites(root)

    if by_owner:
        names = collections.defaultdict(set)
        files = collections.defaultdict(set)
        for name, found in where.items():
            if is_a_script_local(found):
                continue
            for path, _, _, _ in split(found)[0]:
                names[owner_of(path)].add(name)
                files[owner_of(path)].add(path)
        for owner in sorted(names, key=lambda o: -len(names[o])):
            print(f"{owner}: {len(names[owner])} variables over {len(files[owner])} files")
            print("   " + ", ".join(sorted(names[owner])))
            print()
        return

    shown = 0
    locals_seen = 0
    for name in sorted(where):
        reads, writes = split(where[name])
        owners = sorted({owner_of(path) for path, _, _, _ in reads})
        if is_a_script_local(where[name]):
            locals_seen += 1
            if only_owner:
                continue
            print(f"{PREFIX}{name}   [{A_LOCAL}]")
            for path, _, number, _ in where[name]:
                print(f"    {path}:{number}")
            print()
            continue
        if only_owner and only_owner not in owners:
            continue
        shown += 1
        print(f"{PREFIX}{name}   [{', '.join(owners) or NO_READ}]")
        for path, call, number, _ in reads:
            print(f"    read   {path}:{number}  {call}")
        for path, call, number, _ in writes:
            print(f"    write  {path}:{number}  {call}")
        print()
    print(f"{shown} variables, and {locals_seen} name(s) a script keeps to itself")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--root", default=".", help="the repository to read")
    parser.add_argument("--owner", default="", help="only variables a file of this piece reads")
    parser.add_argument("--by-owner", action="store_true", help="one line a piece, for sizing the work")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        survey(args.root, args.owner, args.by_owner)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
