#!/usr/bin/env python
# run-log-kind: analysis

"""Is the first run of a multi-run measurement slower than the rest, and how often?

## What this decides

`tools/measure-menus.py` takes one more run than it was asked for and throws the first away.
That costs a run per pass, and this is what says whether the run is worth it.

## Why the first run of an OLD pass counts as a cold run

Every multi-run folder written before the discard existed has a run-1 that was the first time
that binary, that index and that save were touched in that invocation - which is what a cold
run is. So the whole history is evidence, and the question did not have to wait for new data
to be collected under the new protocol.

Where a folder has a run-cold of its own, that is used instead and run-1 is treated as kept.

## How to read the answer

TWO SEPARATE QUESTIONS, and only the second matters. Whether run-1 is *the slowest* of its
pass is nearly uninformative: in a three-run pass that happens a third of the time by chance,
so anything near 33 per cent means nothing. Whether run-1 stands OUTSIDE the spread of its own
later runs is the real one, because a first run several times beyond its siblings' scatter is
not a coin landing the same way twice.

The distribution found in September 2026 over 54 passes was bimodal: absent in fifty, and 10
to 13 per cent in three whole-game passes whose later runs agreed within 1 to 7. A pass in
that minority is silently wrong rather than visibly noisy, which is the case for discarding.
"""

import argparse
import csv
import statistics
import sys
import traceback

from pathlib import Path

LOGS = Path("performance/logs")
COLD = "run-cold"

###############################################################################
# Core functions
###############################################################################


def run_order(folder):
    """run-1 before run-2 before run-10, rather than by string."""
    tail = folder.name.rsplit("-", 1)[-1]
    return int(tail) if tail.isdigit() else 0


def total_of(rows_file):
    """A run's total menu_ms and how many groups it measured."""
    with rows_file.open(encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle, delimiter="\t"))
    times = [int(r["menu_ms"]) for r in rows if (r.get("menu_ms") or "").isdigit()]
    return sum(times), len(times)


def pass_of(folder):
    """`(cold, kept, groups)` for one folder, or None where it cannot be compared.

    REFUSED WHERE THE RUNS MEASURED DIFFERENT GROUPS, because a resumed or interrupted run
    leaves a total over a different set of menus, and comparing those two totals compares the
    sets rather than the runs.
    """
    runs = sorted(folder.glob("run-*/menus.tsv"), key=lambda p: run_order(p.parent))
    if not runs:
        return None

    named = {p.parent.name: total_of(p) for p in runs}
    if COLD in named:
        cold = named.pop(COLD)
        kept = list(named.values())
    else:
        ordered = [named[p.parent.name] for p in runs]
        cold, kept = ordered[0], ordered[1:]
    if not kept:
        return None

    counts = {count for _, count in [cold, *kept]}
    if len(counts) != 1 or cold[1] == 0:
        return None
    return cold[0], [total for total, _ in kept], cold[1]


def report(logs, cut):
    passes, skipped = [], 0
    for folder in sorted(Path(logs).iterdir()):
        if not folder.is_dir():
            continue
        found = pass_of(folder)
        if found is None:
            if any(folder.glob("run-*/menus.tsv")):
                skipped += 1
            continue
        passes.append((folder.name, *found))

    if not passes:
        print("no comparable multi-run passes found")
        return
    print(f"comparable passes: {len(passes)}")
    print(f"skipped, runs measured different group counts: {skipped}\n")

    slowest = sum(1 for _, cold, kept, _ in passes if cold > max(kept))
    share = 100 * slowest / len(passes)
    print(f"cold run slowest of its pass:  {slowest:3} of {len(passes)}  ({share:.0f}%)")
    print("  a three-run pass does that 33% of the time by chance, so this is the weak reading\n")

    over = []
    for name, cold, kept, groups in passes:
        best = min(kept)
        excess = 100 * (cold - best) / max(1, best)
        spread = 100 * (max(kept) - min(kept)) / max(1, best)
        if excess > cut:
            over.append((name, excess, spread, groups))

    print(
        f"cold run over the fastest kept run by more than {cut}%: {len(over)} of {len(passes)} "
        f"({100 * len(over) / len(passes):.0f}%)"
    )
    if over:
        print("\n  excess  later-run spread  groups  folder")
        for name, excess, spread, groups in sorted(over, key=lambda r: -r[1]):
            print(f"  {excess:5.1f}%  {spread:14.1f}%  {groups:6}  {name[:52]}")
        excesses = [excess for _, excess, _, _ in over]
        print(f"\n  median excess where it appears: {statistics.median(excesses):.1f}%")
        print(
            "  compare each against its own later-run spread: a cold run several times "
            "beyond\n  that scatter is the effect; one inside it is the pass being noisy."
        )


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--logs", default=str(LOGS), help="where the measurement folders are")
    parser.add_argument(
        "--cut",
        type=float,
        default=5.0,
        help="how far over the fastest kept run counts as a slow cold run, in per cent",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        report(args.logs, args.cut)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
