#!/usr/bin/env python

"""Read one matrix run against another, row for row.

Pairs rows on (conv, profile) and reports, per engine, what moved: verdicts first, then the
distribution of the ratios. A row present in one run and not the other is named rather than
dropped - a group that has rows in one folder and none in the other is a finding about the
run, not a gap in the comparison.

COLUMNS ARE MATCHED BY NAME, from each file's own header. The matrix's columns have moved
several times - `fwdbwd` split into `ingame` and `nolimit`, and every engine later grew a
`_setup` - so reading them by position across two runs compares unrelated numbers and
produces a plausible table.
"""

import argparse
import glob
import os
import sys
import traceback

###############################################################################
# Core functions
###############################################################################

# A row that never ran is not a measurement, and pairing one with a real row would report a
# speed-up or a slow-down that never happened.
NOT_A_MEASUREMENT = {"NOT-MEASURED", "CRASHED", "?", ""}

# What counts as a change worth printing rather than noise. A matrix row is a wall clock on
# one machine, so small movements are the machine, not the code.
NOISE = 0.10


def compare(before, after, engine="ingame", noise=NOISE):
    old = read(before, engine)
    new = read(after, engine)

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


def read(folder, engine):
    """Every (conv, profile) -> {column: value} the folder holds for one engine."""
    rows = {}
    for path in sorted(glob.glob(os.path.join(folder, "performance-matrix-*.tsv"))):
        with open(path) as handle:
            header = handle.readline().rstrip("\n").split("\t")
            if f"{engine}_verdict" not in header or "profile" not in header:
                continue
            mine = {
                name[len(engine) + 1 :]: index for index, name in enumerate(header) if name.startswith(f"{engine}_")
            }
            at_conv, at_profile = header.index("conv"), header.index("profile")
            for line in handle:
                cells = line.rstrip("\n").split("\t")
                if len(cells) < len(header):
                    continue
                # THE LAST ROW PER KEY WINS: the files are appended to, so a retried row
                # sits after the one it replaces. The same rule the resume reads by.
                rows[(cells[at_conv], cells[at_profile])] = {name: cells[index] for name, index in mine.items()}
    return rows


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
