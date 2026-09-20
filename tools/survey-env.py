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

A read inside a file that IS the door - `src/core/env.rs`, `tools/degct-env.sh`,
`tools/DegctEnv.psm1` - is the helper explaining or testing itself rather than an option anybody
passes. Those three take a name as a parameter and read nothing of their own, so every name in
them is an example. `tools/measurement_common.py` is NOT excluded: it defines the Python door and
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

# The files ABOUT the variables, skipped entirely. The same four `tests/environment_table.rs`
# skips, and for the same reason: a file whose subject is the list cannot be evidence for it.
ABOUT = frozenset(
    {
        "docs/environment.md",
        "tests/environment_table.rs",
        "tools/survey-env.py",
        "tools/degct-env.sh",
        "tools/DegctEnv.psm1",
    }
)

# The file that is nothing but the Rust door, whose own reads are self-tests. Not in ABOUT,
# because it is ordinary code that happens to test itself rather than a document about the list.
HELPERS_ONLY = frozenset({"src/core/env.rs"})

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
#   A PHANTOM. `DEGCT_PROFILE` appears only in a doc comment in `performance/seen_profile.rs`.
#   Nothing sets it and nothing reads it, and it had a row in `docs/environment.md` anyway,
#   because a prose mention is what puts a row there.
NO_READ = "no recognised read"


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
            # A DOUBLED PREFIX is a name the docs spell out to warn about rather than a variable:
            # callers build names from both halves, and `DEGCT_DEGCT_CONVERSATION` is what that
            # goes wrong as.
            found.append((match.group(1).removeprefix(PREFIX), "spelled-out", number))
    return found


def sites(root):
    """Every asking site in the repository, keyed on the bare variable name."""
    where = collections.defaultdict(list)
    for path in tracked_files(root):
        # THE FILES ABOUT THE VARIABLES, skipped entirely rather than only for their reads: the
        # doc and the test that guards it name every variable by construction, and a door names
        # one to show how it is opened. The same four `tests/environment_table.rs` skips, because
        # this and that are supposed to agree.
        if path in ABOUT:
            continue
        text = (root / path).read_text(encoding="utf-8", errors="replace")
        for name, call, number in asks_in(text):
            where[name].append((path, call, number))
    return where


def split(found):
    """One variable's sites as (reads, writes), prose and self-tests left out of both."""
    reads = [
        site for site in found if site[1] != "spelled-out" and site[1] not in WRITERS and site[0] not in HELPERS_ONLY
    ]
    writes = [site for site in found if site[1] in WRITERS]
    return reads, writes


def survey(root, only_owner, by_owner):
    root = Path(root)
    where = sites(root)

    if by_owner:
        names = collections.defaultdict(set)
        files = collections.defaultdict(set)
        for name, found in where.items():
            for path, _, _ in split(found)[0]:
                names[owner_of(path)].add(name)
                files[owner_of(path)].add(path)
        for owner in sorted(names, key=lambda o: -len(names[o])):
            print(f"{owner}: {len(names[owner])} variables over {len(files[owner])} files")
            print("   " + ", ".join(sorted(names[owner])))
            print()
        return

    shown = 0
    for name in sorted(where):
        reads, writes = split(where[name])
        owners = sorted({owner_of(path) for path, _, _ in reads})
        if only_owner and only_owner not in owners:
            continue
        shown += 1
        print(f"{PREFIX}{name}   [{', '.join(owners) or NO_READ}]")
        for path, call, number in reads:
            print(f"    read   {path}:{number}  {call}")
        for path, call, number in writes:
            print(f"    write  {path}:{number}  {call}")
        print()
    print(f"{shown} variables")


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
