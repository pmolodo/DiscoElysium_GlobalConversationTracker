#!/usr/bin/env python

"""Which options two menu runs star differently, menu by menu.

## Why a diff of LISTS and not of counts

`rounds` says how many options a marking starred and cannot say which. Two markings can
star the same number of options and not the same options, so a change that swapped one
recommendation for another would read as no change at all - which is exactly what a
comparison of two markings must not hide. The matrix's `starred` column carries the entry
ids, and this reads them as sets.

## What it prints

One line per menu that differs, naming what the second run added and what it dropped, then
totals. A menu present in one run and not the other is named rather than passed over: a
group with a row in one folder and none in the other is a finding about the run.

COLUMNS ARE MATCHED BY NAME, from each file's own header: the matrix's columns have moved
several times, and reading them by position across two runs compares unrelated numbers and
produces a plausible table.
"""

import argparse
import os
import sys
import traceback

###############################################################################
# Core functions
###############################################################################

MENUS = "menus.tsv"

# The column naming the menu, and the one holding the starred entry ids.
CONVERSATION = "conv"
STARRED = "starred"

# What the matrix writes where a menu starred nothing.
NOTHING = "-"


def read(folder):
    """Each menu's starred set, by conversation."""
    path = folder if os.path.isfile(folder) else os.path.join(folder, MENUS)
    with open(path, encoding="utf-8") as handle:
        lines = [line.rstrip("\n") for line in handle if line.strip()]

    if not lines:
        raise RuntimeError(f"{path} holds no rows")

    header = lines[0].split("\t")
    for wanted in (CONVERSATION, STARRED):
        if wanted not in header:
            raise RuntimeError(f"{path} has no '{wanted}' column; it has {header}")

    at_conversation = header.index(CONVERSATION)
    at_starred = header.index(STARRED)

    found = {}
    for line in lines[1:]:
        cells = line.split("\t")
        if len(cells) <= max(at_conversation, at_starred):
            continue

        conversation = cells[at_conversation]
        starred = cells[at_starred]
        # A ROW THAT WAS NOT MEASURED has a word where a number goes and '?' after it. It is
        # not a menu that starred nothing, and reading it as one would invent an agreement.
        if starred in ("?", ""):
            continue

        found[conversation] = set() if starred == NOTHING else set(starred.split(","))
    return found


def compare(before, after):
    both = sorted(set(before) & set(after), key=int)
    moved = []
    for conversation in both:
        was, now = before[conversation], after[conversation]
        if was != now:
            moved.append((conversation, sorted(now - was, key=int), sorted(was - now, key=int)))
    return moved


def report(before_folder, after_folder):
    before = read(before_folder)
    after = read(after_folder)

    only_before = sorted(set(before) - set(after), key=int)
    only_after = sorted(set(after) - set(before), key=int)
    if only_before:
        print(f"only in {before_folder}: {', '.join(only_before)}")
    if only_after:
        print(f"only in {after_folder}: {', '.join(only_after)}")

    moved = compare(before, after)
    added_total = dropped_total = 0
    for conversation, added, dropped in moved:
        pieces = []
        if added:
            pieces.append(f"+{','.join(added)}")
        if dropped:
            pieces.append(f"-{','.join(dropped)}")
        added_total += len(added)
        dropped_total += len(dropped)
        print(f"  {conversation:>6}  {' '.join(pieces)}")

    shared = len(set(before) & set(after))
    marks_before = sum(len(one) for one in before.values())
    marks_after = sum(len(one) for one in after.values())
    print()
    print(f"{shared} menus in both, {len(moved)} of them star different options")
    print(f"{added_total} star(s) added, {dropped_total} dropped")
    print(f"{marks_before} marks before, {marks_after} after")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("before", help="a menus run folder, or its menus.tsv")
    parser.add_argument("after", help="the run to read against it")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        report(args.before, args.after)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
