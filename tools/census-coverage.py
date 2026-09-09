#!/usr/bin/env python

"""How much of the game a census can actually classify, per entry.

de-x8ms.5's first question: before designing an artefact that holds a reachability status
per entry, find out how many entries the run data can give one for.

    python tools/census-coverage.py <run>/groups.tsv <run>/census.tsv

THE FIVE BUCKETS, and the fifth must not be folded into the fourth:

  edge-unreachable       no link path reaches it, guards ignored. Static, from the index.
  scenario-unreachable   a link path reaches it, but no path under this world's conditions.
  scenario-reachable     reachable under this world's conditions.
  undecided              a pass ran and did not settle, so nothing is established.
  not covered            no pass ever looked at it. NOT the same as undecided - one is a
                         search that proved nothing, the other is a search that never ran.

## The subtlety this exists to handle

A census names only the entries it proved UNREACHABLE. It never names a reachable one. So
the reachable ones are recovered by SUBTRACTION - everything it looked at and did not name -
and that is sound only where it looked at everything, which is what `exact = all` says.
Where it stopped early (`at-least`, the CENSUS_WANTED cap) the rest were never examined and
belong in `not covered`.

Run a census with DEGCT_CENSUS_ALL=1 to remove that cap; then `not covered` should be zero and
`undecided` is the only thing between the artefact and full coverage.
"""

import argparse
import sys
import traceback

###############################################################################
# Core functions
###############################################################################


def read_groups(path):
    """start -> (conversations, entries, reachable), from a run's groups.tsv."""
    groups = {}
    with open(path, "r", encoding="utf-8") as handle:
        for line in handle:
            cells = line.rstrip("\n").split("\t")
            if len(cells) < 4:
                continue
            groups[int(cells[0])] = (int(cells[1]), int(cells[2]), int(cells[3]))
    return groups


def read_census(path):
    """start -> the census row, skipping the header and any CRASHED group."""
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
                "undecided": int(cells[3]),
                "exact": cells[4],
                "named": [e for e in cells[6].split(",") if e],
            }
    return rows


def coverage(groups, census):
    """The five buckets, plus the counts a reader needs to judge them."""
    measurable = {g: v for g, v in groups.items() if v[2] > 0}

    out = {
        "groups": len(groups),
        "measurable": len(measurable),
        "fully_classified": 0,
        "entries_all": sum(e for (_c, e, _r) in groups.values()),
        "entries_measurable": sum(e for (_c, e, _r) in measurable.values()),
        "edge_unreachable": 0,
        "scenario_unreachable": 0,
        "scenario_reachable": 0,
        "undecided": 0,
        "uncovered": 0,
    }

    for start, (_convs, entries, reachable) in measurable.items():
        row = census.get(start)
        # `candidates` already drops the start itself and the group nodes, so this
        # over-counts by that much rather than being exact. Said rather than hidden.
        out["edge_unreachable"] += entries - reachable
        if row is None:
            out["uncovered"] += reachable
            continue

        named = len(row["named"])
        out["scenario_unreachable"] += named
        if row["exact"] == "all":
            out["undecided"] += row["undecided"]
            out["scenario_reachable"] += row["candidates"] - named - row["undecided"]
            if row["undecided"] == 0:
                out["fully_classified"] += 1
        else:
            out["uncovered"] += row["candidates"] - named

    return out


def report(out):
    print(f"groups in the game            {out['groups']}")
    print(f"  with rows (measurable)      {out['measurable']}")
    print(f"  every candidate settled     {out['fully_classified']}")
    print()
    print(f"entries in the whole game     {out['entries_all']}")
    print(f"entries in measurable groups  {out['entries_measurable']}   <- the artefact's scope")
    print()
    print("per-entry status derivable from what is on disk:")
    for name, key in [
        ("edge-unreachable", "edge_unreachable"),
        ("scenario-unreachable", "scenario_unreachable"),
        ("scenario-reachable", "scenario_reachable"),
        ("undecided", "undecided"),
        ("NOT COVERED BY ANY RUN", "uncovered"),
    ]:
        print(f"  {name:<26} {out[key]:>8}")

    settled = out["scenario_unreachable"] + out["scenario_reachable"] + out["undecided"]
    link_reachable = settled + out["uncovered"]
    if link_reachable:
        share = 100 * settled / link_reachable
        print()
        print(f"{settled} of {link_reachable} link-reachable entries have a world-conditioned")
        print(f"status - {share:.1f}%. The rest were never looked at.")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("groups_tsv", help="a census run's groups.tsv")
    parser.add_argument("census_tsv", help="the same run's census.tsv")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        report(coverage(read_groups(args.groups_tsv), read_census(args.census_tsv)))
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
