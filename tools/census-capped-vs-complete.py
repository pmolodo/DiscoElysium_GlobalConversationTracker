#!/usr/bin/env python

"""Does a capped census name the same entries a complete one names first?

    python tools/census-capped-vs-complete.py <capped>/census.tsv <complete>/census.tsv

## The question this answers

A capped census stops as soon as it has `CENSUS_WANTED` findings, walking the order the
SEARCH defines - novelty class, then link distance from the start, furthest first. A
complete census asks about every candidate and its list is then sorted into the order
`candidates()` defines, by structural depth.

THOSE ARE TWO DIFFERENT DEPTH METRICS, and nothing guarantees they agree. Where they
disagree, a capped run holds ten unreachable entries that are not the ten DEEPEST - the
same count, a different list - and every `deepest-unreach-N` profile built from it asks
about the wrong end of the group. That is not hypothetical: comparing group 436 across two
censuses caught exactly that shape of error once already.

So this takes the first N of the complete run's list, where N is how many the capped run
named, and asks whether they are the same N in the same order.

## What it cannot tell you

Only groups present in BOTH files are compared, and a complete census is expensive, so in
practice that is a handful of groups rather than the whole game. Agreement here is evidence
that the two orders line up, not proof that they always will.

A group whose complete row says `at-least` is not a complete row at all and would make this
comparison meaningless; those are skipped rather than compared.
"""

import argparse
import sys
import traceback

###############################################################################
# Core functions
###############################################################################


def read_census(path):
    """start -> the row's verdict list, skipping the header and any CRASHED group."""
    rows = {}
    with open(path, "r", encoding="utf-8") as handle:
        next(handle)
        for line in handle:
            cells = line.rstrip("\n").split("\t")
            if len(cells) < 7 or cells[1] == "CRASHED":
                continue
            rows[int(cells[0])] = {
                "exact": cells[4],
                "named": [entry for entry in cells[6].split(",") if entry],
            }
    return rows


def compare(capped, complete):
    """Every shared group, and whether the capped list is the complete one's prefix."""
    out = []
    for start in sorted(set(capped) & set(complete)):
        # A complete run that stopped at the cap is not a complete run, and comparing
        # against its prefix would be comparing two capped lists while calling one of them
        # the answer.
        if complete[start]["exact"] != "all":
            continue
        named = capped[start]["named"]
        want = complete[start]["named"][: len(named)]
        out.append((start, want, named))
    return out


def report(compared):
    agreed = 0
    for start, want, named in compared:
        if want == named:
            agreed += 1
            print(f"group {start:>6}  AGREES on {len(named)} entries")
            continue
        print(f"group {start:>6}  DIFFERS")
        print(f"    complete's first {len(want):>3}: {','.join(want)}")
        print(f"    capped named:         {','.join(named)}")
        print(f"    capped missed: {sorted(set(want) - set(named))}")
        print(f"    capped added:  {sorted(set(named) - set(want))}")

    print()
    if not compared:
        print("no group is complete in one file and present in the other")
        return
    print(f"{agreed} of {len(compared)} comparable group(s) agree")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("capped_tsv", help="a capped census run's census.tsv")
    parser.add_argument("complete_tsv", help="a CENSUS_ALL run's census.tsv")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        report(compare(read_census(args.capped_tsv), read_census(args.complete_tsv)))
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
