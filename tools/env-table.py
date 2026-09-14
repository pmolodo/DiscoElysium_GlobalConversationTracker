#!/usr/bin/env python

"""Print the table of DEGCT_ environment variables that docs/environment.md carries.

Generated from the code rather than maintained by hand: for every tracked file it finds the
variables the file names, either spelled out in full (DEGCT_CONVERSATION) or asked for by bare
name through the helper for its language - see the table of helpers in docs/environment.md.
A name read through a FOREIGN door (env::foreign, foreign=True) is somebody else's and is
left out.

Prints one markdown row per variable, sorted, with the files that name it:

    python tools/env-table.py > rows.md
"""

import argparse
import re
import subprocess
import sys
import traceback

from collections import defaultdict
from pathlib import Path

###############################################################################
# Core functions
###############################################################################

ROOT = Path(__file__).resolve().parent.parent

# The file this table lives in, which would otherwise count every variable it lists.
TABLE = "docs/environment.md"

# Where a name can be asked for. Each pattern's one capturing group is the bare name.
NAME = r"([A-Z][A-Z0-9_]*[A-Z0-9])"
PATTERNS = [
    # Spelled out in full, anywhere - code, a comment, a doc. Not the scratch prefix.
    re.compile(r"(?<![A-Za-z0-9_])DEGCT_" + NAME),
    # Rust: env::var("X"), core::env::is_set("X"), env::number("X", ..), env::pass("X", ..),
    # env::qualified("X").
    # Not std::env::var, which reads somebody else's name under its own spelling.
    re.compile(r"(?<!std::)\benv::(?:var|is_set|number|pass|qualified)\(\s*\"" + NAME + r"\""),
    # Rust measurements' own wrappers over those: from_env("X", ..), from_env_i32("X", ..),
    # numbers("X", ..).
    re.compile(r"\b(?:from_env|from_env_i32|numbers)\(\s*\"" + NAME + r"\""),
    # Python: env("X"), env_is_set("X"), env_int("X", ..), env_list("X"), qualified("X").
    re.compile(r"\b(?:env|env_is_set|env_int|env_list|qualified)\(\s*\"" + NAME + r"\"(?![^)]*foreign=True)"),
    # bash: degct_env X, degct_env_is_set X, degct_env_set X.
    re.compile(r"\bdegct_env(?:_is_set|_set)?\s+" + NAME + r"\b"),
    # PowerShell: Get-DegctEnv X, Test-DegctEnv X, Set-DegctEnv X.
    re.compile(r"\b(?:Get|Test|Set)-DegctEnv\s+" + NAME + r"\b"),
    # C#: DegctEnvironment.Get("X"), .IsSet("X"), .Number("X", ..), .Set("X", ..).
    re.compile(r"\bDegctEnvironment\.(?:Get|IsSet|Number|Set)\(\s*\"" + NAME + r"\""),
]

# Python's env_for_child(X=.., Y=..) names each keyword.
FOR_CHILD = re.compile(r"\benv_for_child\(([^)]*)\)")
KEYWORD = re.compile(NAME + r"\s*=")

SUFFIXES = {".rs", ".py", ".sh", ".psm1", ".cs", ".md"}


def tracked_files():
    listed = subprocess.run(
        ["git", "ls-files"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.splitlines()
    # Not the table, which would count every variable it lists, and not this script, whose own
    # docs name variables as examples of what it looks for.
    skipped = {TABLE, Path(__file__).resolve().relative_to(ROOT).as_posix()}
    return [path for path in listed if Path(path).suffix in SUFFIXES and path not in skipped]


def names_in(text):
    found = set()
    for pattern in PATTERNS:
        for match in pattern.finditer(text):
            found.add(match.group(1))
    for call in FOR_CHILD.finditer(text):
        found.update(KEYWORD.findall(call.group(1)))
    return found


def table():
    readers = defaultdict(set)
    for path in tracked_files():
        text = (ROOT / path).read_text(encoding="utf-8", errors="replace")
        for name in names_in(text):
            # A name that already carries the prefix is the doubled spelling the helpers
            # exist to prevent, named in their docs as the mistake - not a variable.
            if not name.startswith("DEGCT"):
                readers[name].add(path)
    rows = []
    for name in sorted(readers):
        files = ", ".join(f"`{path}`" for path in sorted(readers[name]))
        rows.append(f"| `DEGCT_{name}` | {files} |")
    return rows


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    get_parser().parse_args(argv)
    try:
        print("\n".join(table()))
    except Exception:  # pylint: disable=broad-except

        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
