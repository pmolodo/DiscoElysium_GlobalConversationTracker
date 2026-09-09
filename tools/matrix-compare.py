#!/usr/bin/env python

"""Read one matrix run against another, row for row.

Pairs rows on (conv, profile) and reports, per engine, what moved: verdicts first, then the
distribution of the ratios. A row present in one run and not the other is named rather than
dropped - a group that has rows in one folder and none in the other is a finding about the
run, not a gap in the comparison.

COLUMNS ARE MATCHED BY NAME, from each file's own header. The matrix's columns have moved
several times, so reading them by position across two runs compares unrelated numbers and
produces a plausible table. A column one folder has and the other does not simply goes
unpaired.

AND EVERY TOTAL IS REPORTED THREE WAYS - everything, the hardest few groups and the easiest
hundred. A whole-game total is very nearly the sum of the heaviest few groups, so a change
that helps them and hurts five hundred small ones reads as a win and one that helps five
hundred small ones reads as nothing. See `matrix_common`, which also says why the split is
ranked by the census rather than by the run being read.
"""

import argparse
import sys
import traceback

import matrix_common

from matrix_common import NOT_A_MEASUREMENT

###############################################################################
# Core functions
###############################################################################

# What counts as a change worth printing rather than noise. A matrix row is a wall clock on
# one machine, so small movements are the machine, not the code.
NOISE = 0.10


def compare(before, after, engine="ingame", noise=NOISE):
    old = matrix_common.rows_of(before, engine)
    new = matrix_common.rows_of(after, engine)
    # THE BEFORE RUN'S CENSUS, so the buckets are the baseline's opinion of which groups are
    # hard. Either arm's would do - the driver reuses one across a comparison - and picking
    # the baseline's makes it the same one however many arms are read against it.
    census = matrix_common.census_of(before) or matrix_common.census_of(after)

    print(f"before  {before}")
    print(f"after   {after}")
    print(f"engine  {engine}\n")

    shared = sorted(set(old) & set(new))
    only_old = sorted(set(old) - set(new))
    only_new = sorted(set(new) - set(old))
    print(f"{len(shared)} rows in both, {len(only_old)} only before, {len(only_new)} only after")
    if only_new:
        print(f"  only after:  {summarise(only_new)}")
    if only_old:
        print(f"  only before: {summarise(only_old)}")

    verdict_changes(old, new, shared)
    for column in ("ms", "nodes", "asked"):
        movement(old, new, shared, column, noise)

    # LAST, AND READ FIRST. The per-row distribution above says what moved; this says whether
    # what moved was the handful of groups a total is made of or the five hundred it is not.
    for column in ("ms", "nodes"):
        matrix_common.report(old, new, shared, column, census, noise)


def verdict_changes(old, new, shared):
    changed = [key for key in shared if old[key].get("verdict") != new[key].get("verdict")]
    print(f"\nVERDICT changed on {len(changed)} of {len(shared)} rows")
    for key in changed[:40]:
        print(f"  {key[0]:>6} {key[1]:<18} {old[key]['verdict']} -> {new[key]['verdict']}")
    if len(changed) > 40:
        print(f"  ... and {len(changed) - 40} more")

    # `by` says which half of the method answered, and it can move without the verdict
    # moving - which is the same search reaching the same answer a different way.
    moved = [key for key in shared if "by" in old[key] and old[key].get("by") != new[key].get("by")]
    if moved:
        print(f"\nWHICH HALF ANSWERED changed on {len(moved)} rows")
        counts = {}
        for key in moved:
            counts[(old[key]["by"], new[key]["by"])] = counts.get((old[key]["by"], new[key]["by"]), 0) + 1
        for (was, now), count in sorted(counts.items(), key=lambda pair: -pair[1]):
            print(f"  {was} -> {now}: {count}")


def movement(old, new, shared, column, noise):
    pairs = []
    for key in shared:
        if old[key].get("verdict") in NOT_A_MEASUREMENT:
            continue
        if new[key].get("verdict") in NOT_A_MEASUREMENT:
            continue
        try:
            was, now = float(old[key][column]), float(new[key][column])
        except (KeyError, ValueError):
            continue
        pairs.append((key, was, now))

    if not pairs:
        print(f"\n{column}: nothing comparable")
        return

    faster = [p for p in pairs if p[2] < p[1] * (1 - noise)]
    slower = [p for p in pairs if p[2] > p[1] * (1 + noise)]
    was_total = sum(p[1] for p in pairs)
    now_total = sum(p[2] for p in pairs)

    print(f"\n{column}: {len(pairs)} comparable rows")
    print(f"  total       {was_total:>14.0f} -> {now_total:>14.0f}")
    print(f"  median      {median([p[1] for p in pairs]):>14.0f} -> {median([p[2] for p in pairs]):>14.0f}")
    print(f"  max         {max(p[1] for p in pairs):>14.0f} -> {max(p[2] for p in pairs):>14.0f}")
    print(
        f"  moved       {len(slower)} up, {len(faster)} down, "
        f"{len(pairs) - len(slower) - len(faster)} within {noise * 100:.0f}%"
    )

    # The rows that moved most, both ways, because a total hides an exchange.
    worst = sorted(slower, key=lambda p: p[1] - p[2])[:5]
    best = sorted(faster, key=lambda p: p[2] - p[1])[:5]
    for label, rows in (("up most", worst), ("down most", best)):
        for key, was, now in rows:
            print(f"    {label:<9} {key[0]:>6} {key[1]:<18} {was:>10.0f} -> {now:>10.0f}")


def median(values):
    ordered = sorted(values)
    return ordered[len(ordered) // 2] if ordered else 0.0


def summarise(keys, most=12):
    named = [f"{conv}/{profile}" for conv, profile in keys[:most]]
    if len(keys) > most:
        named.append(f"... and {len(keys) - most} more")
    return " ".join(named)


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("before", help="the baseline run folder")
    parser.add_argument("after", help="the run to read against it")
    parser.add_argument("--engine", default="ingame", help="which engine's columns to read")
    parser.add_argument(
        "--noise",
        default=NOISE,
        type=float,
        help="the fraction a row may move before it counts as having moved",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        compare(args.before, args.after, engine=args.engine, noise=args.noise)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
