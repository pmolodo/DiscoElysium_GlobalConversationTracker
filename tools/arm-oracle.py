#!/usr/bin/env python
# run-log-kind: analysis

"""What a PERFECT per-group choice between two measured arms would be worth.

## The question it answers, and why it comes first

Two strategies where neither wins everywhere is the shape of a problem that invites a
heuristic: pick the better one per menu and have both. Building one is a day's work, and the
work is only worth starting if a perfect chooser would win enough to notice.

That bound is cheap to compute and nothing can beat it. Taking the better arm's cost for every
group gives the best any rule could possibly do - including a rule that has already seen the
answers, which no real one has. A heuristic worth building has to come close to it AND beat
both fixed arms; one that beats only the worse arm is a coin flip with extra steps.

IT ALSO SAYS HOW CONCENTRATED THE PRIZE IS. A bound of 200 ms is a different proposition when
one group is 180 of it: that is not a heuristic, it is a special case with a rule wrapped round
it, and it will not survive the group changing.

## What it reads

Two folders written by `tools/measure-menus.py`, the same groups measured both ways. Only the
groups whose `nodes_median` MOVED between the arms are counted: where the node count is
identical the arms did the same work, the difference is the machine, and including them would
dilute the answer with noise. The rest are compared on `menu_ms_median`.

## A threshold sweep, where a feature is named

`--by <column>` also tries every threshold on that column - pick the second arm when the column
is at least t - and prints what each would cost against both arms and against the bound. That is
the simplest rule anyone would write, so if none of its thresholds gets near the bound, the
feature does not separate the two arms and a cleverer rule on the same feature will not either.

Usage:
    tools/arm-oracle.py <folder-a> <folder-b> [--by asked]
"""

import argparse
import csv
import statistics
import sys
import traceback

from pathlib import Path

###############################################################################
# Core functions
###############################################################################

# What a group is judged on, and what says whether the arms did different work at all.
COST = "menu_ms_median"
MOVED = "nodes_median"

# How many of the biggest single differences to name, so a concentrated prize is visible.
BIGGEST = 5


def read(path):
    """One TSV keyed by conversation."""
    with open(path, encoding="utf-8", newline="") as handle:
        return {int(row["conv"]): row for row in csv.DictReader(handle, delimiter="\t")}


def per_run_median(folder, column):
    """The median of a per-run column over a folder's runs, by conversation.

    Some columns a rule would read - how many targets a round asked, say - are written per run
    and not carried into combined.tsv, so they are folded here the same way the costs were.
    """
    seen = {}
    for run in sorted(Path(folder).glob("run-[0-9]*")):
        rows = read(run / "menus.tsv")
        for conversation, row in rows.items():
            value = row.get(column, "")
            if value.lstrip("-").isdigit():
                seen.setdefault(conversation, []).append(int(value))
    return {conv: statistics.median(values) for conv, values in seen.items()}


def number(row, column):
    try:
        return float(row[column])
    except (KeyError, ValueError, TypeError):
        return None


def moved_groups(first, second, by=None):
    """Every group both folders measured whose arms did different work, with what each cost."""
    left, right = read(Path(first) / "combined.tsv"), read(Path(second) / "combined.tsv")
    feature = per_run_median(first, by) if by else {}
    rows = []
    for conversation in sorted(set(left) & set(right)):
        a, b = left[conversation], right[conversation]
        cost_a, cost_b = number(a, COST), number(b, COST)
        nodes_a, nodes_b = number(a, MOVED), number(b, MOVED)
        if None in (cost_a, cost_b, nodes_a, nodes_b) or nodes_a == nodes_b:
            continue
        rows.append(
            {
                "conv": conversation,
                "a": cost_a,
                "b": cost_b,
                "delta": cost_b - cost_a,
                "feature": feature.get(conversation),
            }
        )
    return rows


def report(first, second, by=None):
    rows = moved_groups(first, second, by)
    if not rows:
        raise SystemExit("no group's node count moved between these folders: the arms did the same work")

    total_a = sum(row["a"] for row in rows)
    total_b = sum(row["b"] for row in rows)
    oracle = sum(min(row["a"], row["b"]) for row in rows)
    # THE GROUP THAT CONTRIBUTES MOST OF THE PRIZE, measured against the FIRST arm, which is the
    # one the sentence below compares to: a group where the first arm already wins contributes
    # nothing to beating it.
    best = max(rows, key=lambda row: row["a"] - min(row["a"], row["b"]))

    print(f"{len(rows)} groups where the arms did different work")
    print(f"  {Path(first).name}: {total_a:,.0f} ms")
    print(f"  {Path(second).name}: {total_b:,.0f} ms")
    print(f"  a PERFECT per-group choice: {oracle:,.0f} ms")
    print(
        f"  which is {total_a - oracle:,.0f} ms better than the first arm and "
        f"{total_b - oracle:,.0f} ms better than the second - THE MOST ANY RULE CAN WIN"
    )
    print(
        f"  of what it wins over the first arm, {best['a'] - min(best['a'], best['b']):,.0f} ms is "
        f"conversation {best['conv']} alone ({best['a']:,.0f} against {best['b']:,.0f})\n"
    )

    print(f"the {BIGGEST} biggest differences either way:")
    print(f"  {'conv':>6}  {'first':>8}  {'second':>8}  {'delta':>8}")
    for row in sorted(rows, key=lambda row: -abs(row["delta"]))[:BIGGEST]:
        print(f"  {row['conv']:>6}  {row['a']:>8,.0f}  {row['b']:>8,.0f}  {row['delta']:>+8,.0f}")
    print()

    if by:
        sweep(rows, by, total_a, total_b, oracle)


def sweep(rows, by, total_a, total_b, oracle):
    """Every threshold on `by`: take the second arm where the column is at least t."""
    known = sorted({row["feature"] for row in rows if row["feature"] is not None})
    if not known:
        print(f"no group carries a {by!r} column; nothing to sweep")
        return

    print(f"take the second arm when {by} >= t:")
    print(f"  {'t':>6}  {'total':>9}  {'vs first':>9}  {'vs second':>10}  {'short of perfect':>17}")
    for threshold in known:
        total = sum(row["b"] if (row["feature"] or 0) >= threshold else row["a"] for row in rows)
        print(
            f"  {threshold:>6,.0f}  {total:>9,.0f}  {total - total_a:>+9,.0f}  "
            f"{total - total_b:>+10,.0f}  {total - oracle:>+17,.0f}"
        )


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("first", help="a folder written by tools/measure-menus.py")
    parser.add_argument("second", help="the same groups, measured the other way")
    parser.add_argument(
        "--by",
        help="a per-run column to sweep thresholds on, e.g. asked",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        report(args.first, args.second, by=args.by)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
