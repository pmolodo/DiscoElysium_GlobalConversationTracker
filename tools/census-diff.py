#!/usr/bin/env python

"""What changed between two censuses, per group.

    python tools/census-diff.py <older>/census.tsv <newer>/census.tsv

Written for de-x8ms.4, where fixing the classifier's layout meant every census taken before
it undercounted - and the question "by how much, and where" could not be answered by the
totals. It could not: the skip-rule split barely moved (125/32/364 became 121/29/371) while
fifteen groups changed and one of them went from four unreachable entries to ten.

TWO THINGS IT PRINTS THAT A TOTAL CANNOT. Whether any group went DOWN - a stricter
classifier must only ever find more, so a decrease means the change did something other than
what it claimed - and whether the LIST changed where the count did not, which is what caught
the ordering bug in de-x8ms.11: group 436 kept its ten and returned an entirely different
ten, and only a list comparison showed it.
"""

import argparse
import sys
import traceback

###############################################################################
# Core functions
###############################################################################


def read(path):
    rows = {}
    with open(path, "r", encoding="utf-8") as handle:
        next(handle)
        for line in handle:
            cells = line.rstrip("\n").split("\t")
            if len(cells) < 7 or cells[1] == "CRASHED":
                continue
            rows[int(cells[0])] = {
                "candidates": int(cells[1]),
                "unreachable": int(cells[2]),
                "exact": cells[4],
                "list": [e for e in cells[6].split(",") if e],
            }
    return rows


def compare(old, new, show=8):
    both = sorted(set(old) & set(new))
    print(f"groups in both: {len(both)}  (old {len(old)}, new {len(new)})")

    moved = []
    same_count = list_changed = now_some = 0
    for group in both:
        before, after = old[group], new[group]
        delta = after["unreachable"] - before["unreachable"]
        if delta:
            moved.append((delta, group, before["unreachable"], after["unreachable"]))
        else:
            same_count += 1
        if before["list"] != after["list"]:
            list_changed += 1
        if before["unreachable"] == 0 and after["unreachable"] > 0:
            now_some += 1

    up = [m for m in moved if m[0] > 0]
    down = [m for m in moved if m[0] < 0]

    print("\nunreachable COUNT per group")
    print(f"  went up            {len(up)}")
    print(f"  went down          {len(down)}")
    print(f"  unchanged          {same_count}")
    print(f"\nthe deepest-unreachable LIST differs in {list_changed} group(s)")
    print(f"groups that had none and now have some: {now_some}")

    up.sort(reverse=True)
    if up:
        print("\nlargest increases:")
        for delta, group, before, after in up[:show]:
            print(f"  group {group:>5}: {before} -> {after}   (+{delta})")

    if down:
        print("\nDECREASES - a stricter classifier finding FEWER wants explaining:")
        for delta, group, before, after in sorted(down)[:show]:
            print(f"  group {group:>5}: {before} -> {after}   ({delta})")
    else:
        print("\nno group reported fewer, which is the expected direction.")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("older", help="the earlier run's census.tsv")
    parser.add_argument("newer", help="the later run's census.tsv")
    parser.add_argument("--show", default=8, type=int, help="how many movers to name")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        compare(read(args.older), read(args.newer), args.show)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
