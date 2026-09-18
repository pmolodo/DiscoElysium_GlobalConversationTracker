#!/usr/bin/env python
# run-log-kind: analysis

"""Sort run logs into a folder per date, and keep them that way.

## Why

`performance/logs` had 1,669 loose transcripts and 179 row folders in one directory, and
`testing/logs` another 877. A directory that size is one nobody browses: finding the run from
a particular afternoon means sorting a wall of names, and a shell completing on it is useless.
The names already begin with the date, so the information to group by was there all along.

## How a date is decided

FROM THE NAME WHERE THERE IS ONE. Every run log is written as `<date>_<time>_<revision>_...`
by `tools/run-logged.sh` and `RunLog.cs`, so the date it was taken is in the first field and
is exactly right.

FROM THE MODIFICATION TIME OTHERWISE. A row folder named for what it measured - `warmup-ramp`,
`qy5t-before` - carries no date, and its mtime is when its last row was written, which is the
day the run happened. That is the best available answer and it is a good one; the alternative
is leaving precisely the folders a person names by hand scattered at the top level.

## Idempotent, and safe to run at any time

A file already inside a date folder is left alone, so this can run repeatedly. A move that
would overwrite something is refused rather than resolved, because two runs with the same name
in one day is a thing to look at rather than to silently pick a winner for.
"""

import argparse
import re
import shutil
import sys
import traceback

from datetime import datetime
from pathlib import Path

LOGS = (Path("performance/logs"), Path("testing/logs"))

# The date a run log's name begins with, as run-logged.sh and RunLog.cs write it.
DATED = re.compile(r"^(\d{4}-\d{2}-\d{2})[_-]")

# A folder that is already a date folder, so its contents are where they belong.
DATE_FOLDER = re.compile(r"^\d{4}-\d{2}-\d{2}$")

###############################################################################
# Core functions
###############################################################################


def date_of(entry):
    """The date to file `entry` under: its name's own, or the day it was last written."""
    found = DATED.match(entry.name)
    if found:
        return found.group(1)
    return datetime.fromtimestamp(entry.stat().st_mtime).strftime("%Y-%m-%d")


def moves_for(folder):
    """Every `(entry, destination)` this folder needs, skipping what is already sorted."""
    planned = []
    for entry in sorted(folder.iterdir()):
        if entry.is_dir() and DATE_FOLDER.match(entry.name):
            continue
        planned.append((entry, folder / date_of(entry) / entry.name))
    return planned


def tidy(folders, dry_run):
    for folder in folders:
        folder = Path(folder)
        if not folder.is_dir():
            print(f"{folder}: not there, skipped")
            continue

        planned = moves_for(folder)
        if not planned:
            print(f"{folder}: already sorted")
            continue

        by_date = {}
        for _entry, destination in planned:
            by_date[destination.parent.name] = by_date.get(destination.parent.name, 0) + 1
        print(f"{folder}: {len(planned)} entr(ies) into {len(by_date)} date folder(s)")
        for date in sorted(by_date):
            print(f"  {date}  {by_date[date]:>5}")

        if dry_run:
            continue
        for entry, destination in planned:
            if destination.exists():
                print(f"  REFUSED, something is already there: {destination}", file=sys.stderr)
                continue
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(entry), str(destination))
        print(f"  moved {len(planned)}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "folders",
        nargs="*",
        default=[str(folder) for folder in LOGS],
        help="the log folders to sort",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="say what would move and move nothing",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        tidy(args.folders, args.dry_run)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
