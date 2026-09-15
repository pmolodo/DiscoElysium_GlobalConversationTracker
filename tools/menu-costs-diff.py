#!/usr/bin/env python

"""What two several-run menu measurements cost, group by group, and which groups changed marks.

## What it reads

The `combined.tsv` that `tools/measure-menus.py --runs N` writes into each folder: every group's
median, min and max `menu_ms`, its median nodes, and its rounds, settled and starred, with a flag
for whether every run agreed. Medians are compared because a single run's milliseconds are a
reading of the machine as much as of the search. A median over an even number of runs can fall
between two readings, so it is read as a number rather than as a whole one.

## Refused across different settings

Each folder's `run.json`, which `tools/measure-menus.py` writes, says what the run was: its
command line, its code, its parallelism, every DEGCT_ variable that was set, its run count, and
the machine. Only the variables known to change a menu measurement are compared -
`measurement_common.COMPARED_VARIABLES`, the memory budget among them - so a variable that was
recorded but that nothing reads cannot refuse a comparison. Two folders that differ in a compared
setting differ by more than their code: groups measured side by side pay a flat cost in setup
that groups measured alone do not, measured 2026-09-14 at about ten milliseconds per small group,
and a different budget is a different search. So a comparison across different settings, or with
a folder that has no record, is refused unless `--allow-mixed-settings` says the difference is
known and wanted. Which fields count is `measurement_common.settings_of`, which the driver's
resume check reads too.

## Refused across different hardware, separately

A different machine - hostname, cores, total memory - or available memory at the start more than
`measurement_common.AVAILABLE_MEMORY_TOLERANCE` apart is refused unless `--allow-mixed-hardware`
says so. It is its own flag because it is its own question: two settings can be matched by
re-running, and two machines cannot. The count of running processes is recorded but not compared;
it is a reading of the moment rather than of the machine.

## A different algorithm is warned about, not refused

`DEGCT_MARKING` and the rest of `measurement_common.ALGORITHM_VARIABLES` differ between two folders
exactly when two algorithms are being compared, which is often the point. The difference is printed
as a WARNING above the numbers, so they are not read as one algorithm's code changing.

## What it prints

The two codes; what differs in settings, hardware and algorithm, where anything was allowed to; the
sum of medians over the groups both folders measured; the groups whose runs disagreed in the later
folder; the most slowed and the most sped-up groups; and every group whose rounds, settled or
starred changed. A group present in only one folder, or measured on only one side - its median a
word such as NO-MENU on the other - is named rather than passed over.

`starred` is compared as the text the matrix wrote. Where the question is WHICH options two runs
star, use `tools/menu-marks-diff.py`, which reads them as sets.

COLUMNS ARE MATCHED BY NAME, from each file's own header, for the reason
`tools/menu-marks-diff.py` gives: the matrix's columns have moved, and reading them by position
across two runs compares unrelated numbers.

Usage:
    tools/menu-costs-diff.py [--allow-mixed-settings] [--allow-mixed-hardware] <before> <after>
"""

import argparse
import csv
import json
import sys
import traceback

from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from measurement_common import (  # noqa: E402  (after the path is set)
    RUN_RECORD,
    algorithm_differences,
    hardware_differences,
    setting_differences,
)

###############################################################################
# Core functions
###############################################################################

COMBINED = "combined.tsv"

# The flags that allow a comparison across what would otherwise refuse it.
ALLOW_MIXED_SETTINGS = "--allow-mixed-settings"
ALLOW_MIXED_HARDWARE = "--allow-mixed-hardware"

# How many of the most slowed and most sped-up groups to list.
TOP = 15

# The columns that say what a menu marked rather than what it cost.
MARK_COLUMNS = ("rounds", "settled", "starred")


class Refused(Exception):
    """Two folders differ in something a flag has to allow, and it was not allowed.

    `messages` holds one refusal per kind of difference - settings, hardware - each naming the
    fields that differed, so a reader knows which flag answers which.
    """

    def __init__(self, messages):
        super().__init__("\n".join(messages))
        self.messages = messages


def read(folder):
    path = Path(folder) / COMBINED
    with path.open(newline="") as handle:
        return {row["conv"]: row for row in csv.DictReader(handle, delimiter="\t")}


def run_record(folder):
    """The folder's run record, or None where it has none."""
    path = Path(folder) / RUN_RECORD
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def allowed_or_refused(what, differences, flag, allowed):
    """Prints differences a flag allowed; returns them as one refusal naming each field where it did not."""
    if not differences:
        print(f"{what}: the same")
        return []
    if allowed:
        print(f"COMPARING ANYWAY ({flag}), across different {what}:")
        for line in differences:
            print(f"  {line}")
        return []
    return [
        f"REFUSED ({what}): these {what} fields differ - {'; '.join(differences)}. "
        f"Re-measure one to match the other, or pass {flag} to compare anyway."
    ]


def check_records(before_folder, after_folder, allow_settings, allow_hardware):
    """Refuses two folders that differ in settings or hardware, unless each is allowed."""
    before, after = run_record(before_folder), run_record(after_folder)
    missing = [label for label, record in (("before", before), ("after", after)) if record is None]
    if missing:
        unknown = [
            f"the {label} folder has no {RUN_RECORD}, so what it was measured under is unknown" for label in missing
        ]
        refusals = allowed_or_refused("settings", unknown, ALLOW_MIXED_SETTINGS, allow_settings)
        refusals += allowed_or_refused("hardware", unknown, ALLOW_MIXED_HARDWARE, allow_hardware)
    else:
        for label, record in (("before", before), ("after", after)):
            code = record.get("code") or {}
            print(
                f"code {label}: {code.get('revision')}"
                + (f" dirty, tree {code.get('tree')}" if code.get("dirty") else "")
            )
        # A DIFFERENT ALGORITHM IS ALLOWED, since comparing two is often the point, but said loudly:
        # a reader of the numbers below must not take them for one algorithm's code changing.
        for line in algorithm_differences(before, after):
            print(f"WARNING: a different algorithm: {line}")
        refusals = allowed_or_refused(
            "settings", setting_differences(before, after), ALLOW_MIXED_SETTINGS, allow_settings
        )
        refusals += allowed_or_refused(
            "hardware", hardware_differences(before, after), ALLOW_MIXED_HARDWARE, allow_hardware
        )

    if refusals:
        raise Refused(refusals)


def median(row):
    """The median menu_ms, or None where the group did not measure.

    A group that did not measure carries the driver's word for why in place of a number -
    NO-MENU, CRASHED, NOT-MEASURED - and is compared by that word rather than by a cost.
    """
    try:
        return float(row["menu_ms_median"])
    except ValueError:
        return None


def compare(before_folder, after_folder, allow_settings=False, allow_hardware=False):
    print(f"before: {before_folder}")
    print(f"after:  {after_folder}")
    check_records(before_folder, after_folder, allow_settings, allow_hardware)

    before = read(before_folder)
    after = read(after_folder)
    common = sorted(set(before) & set(after), key=int)

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
        if (median(before[conv]) is None) != (median(after[conv]) is None)
        or (median(before[conv]) is None and before[conv]["menu_ms_median"] != after[conv]["menu_ms_median"])
    ]
    print(f"groups measured on one side only: {len(outcomes)}")
    for conv in outcomes:
        print(f"  {conv}: {before[conv]['menu_ms_median']} -> {after[conv]['menu_ms_median']}")

    shared = [conv for conv in common if median(before[conv]) is not None and median(after[conv]) is not None]
    print(f"groups measured in both: {len(shared)}")

    total_before = sum(median(before[conv]) for conv in shared)
    total_after = sum(median(after[conv]) for conv in shared)
    change = (total_after - total_before) / total_before * 100 if total_before else 0.0
    print(f"sum of medians over shared groups: {total_before:,.1f} -> {total_after:,.1f} ms ({change:+.1f}%)")

    unsteady = [conv for conv in shared if after[conv].get("steady") != "yes"]
    print(f"groups whose runs disagreed after: {', '.join(unsteady) if unsteady else 'none'}")

    deltas = sorted(
        ((median(after[conv]) - median(before[conv]), conv) for conv in shared),
        reverse=True,
    )
    header = f"    {'conv':>6} {'before':>9} {'after':>9} {'delta':>9}  {'nodes before':>13} {'nodes after':>12}"
    for title, rows in (("slowed", deltas[:TOP]), ("sped-up", sorted(deltas)[:TOP])):
        print(f"\nthe {TOP} most {title} groups by median menu_ms:")
        print(header)
        for delta, conv in rows:
            print(
                f"    {conv:>6} {median(before[conv]):>9,.1f} {median(after[conv]):>9,.1f} "
                f"{delta:>+9,.1f}  {float(before[conv]['nodes_median']):>13,.0f} "
                f"{float(after[conv]['nodes_median']):>12,.0f}"
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
    parser.add_argument(
        ALLOW_MIXED_SETTINGS,
        action="store_true",
        help="compare folders measured under different settings, or without a run record",
    )
    parser.add_argument(
        ALLOW_MIXED_HARDWARE,
        action="store_true",
        help="compare folders measured on different hardware, or with too different available memory",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        compare(
            args.before,
            args.after,
            allow_settings=args.allow_mixed_settings,
            allow_hardware=args.allow_mixed_hardware,
        )
    except Refused as refused:
        # ONE LINE PER KIND OF DIFFERENCE, each naming its fields and the flag that answers it.
        for message in refused.messages:
            print(message, file=sys.stderr)
        return 2
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
