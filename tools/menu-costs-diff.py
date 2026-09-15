#!/usr/bin/env python

"""What two several-run menu measurements cost, group by group, and which groups changed marks.

## What it reads

The `combined.tsv` that `tools/measure-menus.py --runs N` writes into each folder: every group's
median, min and max `menu_ms`, its median nodes, and its rounds, settled and starred, with a flag
for whether every run agreed. Medians are compared because a single run's milliseconds are a
reading of the machine as much as of the search.

## What it prints

The sum of medians over the groups both folders measured, the groups whose runs disagreed in the
later folder, the most slowed and the most sped-up groups, and every group whose rounds, settled
or starred changed. A group present in only one folder, or measured on only one side - its median
a word such as NO-MENU on the other - is named rather than passed over.

`starred` is compared as the text the matrix wrote. Where the question is WHICH options two runs
star, use `tools/menu-marks-diff.py`, which reads them as sets.

COLUMNS ARE MATCHED BY NAME, from each file's own header, for the reason
`tools/menu-marks-diff.py` gives: the matrix's columns have moved, and reading them by position
across two runs compares unrelated numbers.

Usage:
    tools/menu-costs-diff.py <before measure_menus folder> <after measure_menus folder>
"""

import argparse
import csv
import sys
import traceback

from pathlib import Path

###############################################################################
# Core functions
###############################################################################

COMBINED = "combined.tsv"

# How many of the most slowed and most sped-up groups to list.
TOP = 15

# The columns that say what a menu marked rather than what it cost.
MARK_COLUMNS = ("rounds", "settled", "starred")


def read(folder):
    path = Path(folder) / COMBINED
    with path.open(newline="") as handle:
        return {row["conv"]: row for row in csv.DictReader(handle, delimiter="\t")}


def median(row):
    """The median menu_ms, or None where the group did not measure.

    A group that did not measure carries the driver's word for why in place of a number -
    NO-MENU, CRASHED, NOT-MEASURED - and is compared by that word rather than by a cost.
    """
    value = row["menu_ms_median"]
    return int(value) if value.isdigit() else None


def compare(before_folder, after_folder):
    before = read(before_folder)
    after = read(after_folder)
    common = sorted(set(before) & set(after), key=int)

    print(f"before: {before_folder}")
    print(f"after:  {after_folder}")
    print(f"groups: {len(before)} before, {len(after)} after, {len(common)} in both")
    for label, only in (
        ("only before", set(before) - set(after)),
        ("only after", set(after) - set(before)),
    ):
        if only:
            print(f"  {label}: {', '.join(sorted(only, key=int))}")

    outcomes = [
        conv
        for conv in common
        if (median(before[conv]) is None or median(after[conv]) is None)
        and before[conv]["menu_ms_median"] != after[conv]["menu_ms_median"]
    ]
    print(f"groups measured on one side only: {len(outcomes)}")
    for conv in outcomes:
        print(f"  {conv}: {before[conv]['menu_ms_median']} -> {after[conv]['menu_ms_median']}")

    shared = [conv for conv in common if median(before[conv]) is not None and median(after[conv]) is not None]
    print(f"groups measured in both: {len(shared)}")

    total_before = sum(median(before[conv]) for conv in shared)
    total_after = sum(median(after[conv]) for conv in shared)
    change = (total_after - total_before) / total_before * 100 if total_before else 0.0
    print(f"sum of medians over shared groups: {total_before:,} -> {total_after:,} ms ({change:+.1f}%)")

    unsteady = [conv for conv in shared if after[conv].get("steady") != "yes"]
    print(f"groups whose runs disagreed after: {', '.join(unsteady) if unsteady else 'none'}")

    deltas = sorted(
        ((median(after[conv]) - median(before[conv]), conv) for conv in shared),
        reverse=True,
    )
    header = f"    {'conv':>6} {'before':>8} {'after':>8} {'delta':>8}  {'nodes before':>13} {'nodes after':>12}"
    for title, rows in (("slowed", deltas[:TOP]), ("sped-up", sorted(deltas)[:TOP])):
        print(f"\nthe {TOP} most {title} groups by median menu_ms:")
        print(header)
        for delta, conv in rows:
            print(
                f"    {conv:>6} {median(before[conv]):>8,} {median(after[conv]):>8,} "
                f"{delta:>+8,}  {int(before[conv]['nodes_median']):>13,} "
                f"{int(after[conv]['nodes_median']):>12,}"
            )

    marked = [conv for conv in shared if any(before[conv][column] != after[conv][column] for column in MARK_COLUMNS)]
    print(f"\ngroups whose rounds, settled or starred changed: {len(marked)}")
    for conv in marked:
        print(f"  {conv}:")
        for column in MARK_COLUMNS:
            if before[conv][column] != after[conv][column]:
                print(f"    {column}: {before[conv][column]} -> {after[conv][column]}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("before", help="the earlier measure_menus folder")
    parser.add_argument("after", help="the later measure_menus folder")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        compare(args.before, args.after)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
