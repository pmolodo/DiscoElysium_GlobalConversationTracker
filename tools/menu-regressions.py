#!/usr/bin/env python

"""Flag performance regressions in a several-run menu measurement, against a baseline marked for it.

## Why a marked baseline, and not a search of old runs

`measurements/logs` holds every run anybody took: trees mid-change, opt-in algorithms, a few groups
rather than all of them, groups measured side by side and one at a time. A search would have to
guess which of those says how fast the shipped code was, and a wrong guess reports a regression that
is not there, or hides one that is, with nothing to say it guessed. So a person marks a run as a
baseline when they know it is one, and a later run is held to the newest baseline measured the same
way as it was.

## Where a baseline lives, and why not in git

`measurements/README.md` gives the reason nothing a measurement produced is committed: a row is a
wall-clock time on one machine, and a committed baseline goes stale silently. So `mark` COPIES what
it needs of a run - `combined.tsv`, `run.json` and `summary.txt` - under
`measurements/logs/baselines/`, which git ignores. It belongs to the machine it was measured on, and
is refused on any other; the copy means a baseline survives its run folder being tidied away.

## What a baseline has to be

- SEVERAL RUNS, at least `MIN_RUNS`, because the rule below compares ranges and a single run has no
  range to compare.
- A RUN RECORD, since without one nothing can say whether a later run was measured the same way.
- A CLEAN TREE. A baseline stands for a commit a later run can be measured against again, and a
  dirty tree is code no commit holds.
- THE SHIPPED ALGORITHM, no `measurement_common.ALGORITHM_VARIABLES` set. A regression is the
  product getting slower; an opt-in arm measured as a baseline would make every later default run
  look like a change.

## Which baseline a run is checked against

The newest marked baseline, by when its run started, whose settings, algorithm and hardware match the
run's - the checks `measurement_common` gives `tools/menu-costs-diff.py`. `--baseline` names one
instead, and it is held to the same checks. With no match, the check is REFUSED and says what
differed for each baseline, rather than comparing against the nearest one.

## What counts as a regression

Calibrated 2026-09-15 on two whole-game measurements of the same engine code, three runs each, one
group at a time (217d690 against f3aab81): the sum of medians moved by 2.5%; the worst group slowed by
13 ms, from 38 ms; groups under 50 ms often moved by 9 ms, which is half again on a 17 ms menu; and
the heaviest groups moved by under a tenth.

So A GROUP is flagged only where all three hold:

- its median rose by at least `GROUP_RELATIVE` of the baseline's median,
- and by at least `GROUP_ABSOLUTE_MS`, which sits above the 13 ms a small group moved by on its own,
- and its FASTEST run is slower than the baseline's SLOWEST, so the two sets of runs do not overlap.

THE WHOLE RUN is flagged where the sum of medians over the groups both measured rose by at least
`TOTAL_RELATIVE`, four times what two runs of the same code drifted apart by.

A group the baseline measured that the run crashed on, or could not measure, is flagged as well. A
group measured on one side only because the run chose different groups is listed, not flagged.

Exit codes: 0 nothing flagged, 1 an error, 2 refused, 3 a regression flagged.

Usage:
    tools/menu-regressions.py mark <folder> [--note TEXT]
    tools/menu-regressions.py list
    tools/menu-regressions.py check <folder> [--baseline NAME]
"""

import argparse
import json
import shutil
import sys
import traceback

from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from measurement_common import (  # noqa: E402  (after the path is set)
    ALGORITHM_VARIABLES,
    COMBINED,
    OUT,
    RUN_RECORD,
    SUMMARY,
    algorithm_differences,
    hardware_differences,
    median_ms,
    read_combined,
    read_run_record,
    setting_differences,
    write_lf,
)

###############################################################################
# Core functions
###############################################################################

# Where marked baselines are copied to. Under logs, which git ignores - see the module note.
BASELINES = OUT / "logs" / "baselines"

# What marking a baseline records about itself, beside what it copied.
BASELINE_RECORD = "baseline.json"

# What of a run a baseline keeps.
COPIED = (COMBINED, RUN_RECORD, SUMMARY)

# The fewest runs a baseline may hold. See the module note.
MIN_RUNS = 3

# A group's regression thresholds, and the whole run's. See the module note for the calibration.
GROUP_RELATIVE = 0.25
GROUP_ABSOLUTE_MS = 20.0
TOTAL_RELATIVE = 0.10

# Exit codes beyond the template's 0 and 1.
EXIT_REFUSED = 2
EXIT_REGRESSED = 3


class Refused(Exception):
    """What was asked cannot honestly be done, and why."""


def record_of(folder):
    """A run folder's run record, refusing a folder that has none."""
    record = read_run_record(folder)
    if record is None:
        raise Refused(f"{folder} has no {RUN_RECORD}, so what it measured is unknown")
    return record


def mark(folder, baselines, note):
    """Copies a run into the baselines, refusing one that cannot stand as a baseline."""
    folder = Path(folder)
    if not (folder / COMBINED).exists():
        raise Refused(f"{folder} has no {COMBINED}; only a several-run measurement can be a baseline")

    record = record_of(folder)
    problems = []

    code = record.get("code") or {}
    if not code.get("revision") or code.get("dirty"):
        problems.append("it was measured on a tree with uncommitted changes, which no commit holds")

    environment = record.get("environment") or {}
    arms = [f"{name}={environment[name]}" for name in sorted(ALGORITHM_VARIABLES) if name in environment]
    if arms:
        problems.append(f"it measured an opt-in algorithm ({', '.join(arms)}) rather than the shipped one")

    fewest = min(int(row["runs"]) for row in read_combined(folder).values())
    if fewest < MIN_RUNS:
        problems.append(f"some group was measured {fewest} time(s), and a baseline needs {MIN_RUNS}")

    if problems:
        raise Refused(f"{folder} cannot be a baseline: " + "; ".join(problems))

    destination = Path(baselines) / folder.name
    if destination.exists():
        raise Refused(f"{folder.name} is already a baseline, at {destination}")

    destination.mkdir(parents=True)
    for name in COPIED:
        if (folder / name).exists():
            shutil.copyfile(folder / name, destination / name)
    marked = {
        "marked": datetime.now().astimezone().isoformat(timespec="seconds"),
        "note": note,
        "source": str(folder.resolve()),
    }
    write_lf(destination / BASELINE_RECORD, json.dumps(marked, indent=2) + "\n")
    print(f"marked {folder} as a baseline, copied to {destination}")


def marked_baselines(baselines):
    """Every marked baseline, newest run first."""
    root = Path(baselines)
    if not root.exists():
        return []
    found = [path for path in root.iterdir() if (path / BASELINE_RECORD).exists()]
    return sorted(found, key=lambda path: record_of(path).get("started") or "", reverse=True)


def describe(path):
    """One line saying what a baseline is."""
    record = record_of(path)
    marked = json.loads((path / BASELINE_RECORD).read_text(encoding="utf-8"))
    code = (record.get("code") or {}).get("revision") or "?"
    note = f" - {marked['note']}" if marked.get("note") else ""
    return (
        f"{path.name}: {code[:12]}, started {record.get('started')}, "
        f"parallelism {record.get('parallelism')}, marked {marked.get('marked')}{note}"
    )


def list_baselines(baselines):
    found = marked_baselines(baselines)
    if not found:
        print(f"no baselines in {baselines}")
    for path in found:
        print(describe(path))


def differences(baseline, record):
    """Every way a baseline was measured differently from a run, one line each."""
    return (
        setting_differences(baseline, record)
        + algorithm_differences(baseline, record)
        + hardware_differences(baseline, record)
    )


def choose(record, baselines, named):
    """The baseline a run is held to: the named one, or the newest measured the same way."""
    candidates = [Path(baselines) / named] if named else marked_baselines(baselines)
    if named and not (candidates[0] / BASELINE_RECORD).exists():
        raise Refused(f"there is no baseline called {named} in {baselines}")

    rejected = []
    for path in candidates:
        lines = differences(record_of(path), record)
        if not lines:
            return path
        rejected.append(f"{path.name}: {'; '.join(lines)}")

    if not rejected:
        raise Refused(f"there are no baselines in {baselines}; mark one with `mark`")
    raise Refused("no baseline was measured the way this run was:\n  " + "\n  ".join(rejected))


def regressed_group(before, after):
    """Whether one group measured on both sides is a regression, by the rule in the module note."""
    was, now = median_ms(before), median_ms(after)
    rise = now - was
    return (
        rise >= GROUP_ABSOLUTE_MS
        and rise >= GROUP_RELATIVE * was
        and float(after["menu_ms_min"]) > float(before["menu_ms_max"])
    )


def check(folder, baselines, named=None):
    """Holds a run to its baseline and prints what regressed; returns whether anything did."""
    folder = Path(folder)
    record = record_of(folder)
    baseline = choose(record, baselines, named)
    print(f"run:      {folder}")
    print(f"baseline: {describe(baseline)}")

    before, after = read_combined(baseline), read_combined(folder)
    common = sorted(set(before) & set(after), key=int)
    only = {
        "baseline only": sorted(set(before) - set(after), key=int),
        "run only": sorted(set(after) - set(before), key=int),
    }
    for label, groups in only.items():
        if groups:
            print(f"groups in the {label}, not compared: {', '.join(groups)}")

    shared = [conv for conv in common if median_ms(before[conv]) is not None and median_ms(after[conv]) is not None]
    lost = [conv for conv in common if median_ms(before[conv]) is not None and median_ms(after[conv]) is None]
    slowed = [conv for conv in shared if regressed_group(before[conv], after[conv])]

    total_before = sum(median_ms(before[conv]) for conv in shared)
    total_after = sum(median_ms(after[conv]) for conv in shared)
    total_rise = (total_after - total_before) / total_before if total_before else 0.0
    total_regressed = total_rise >= TOTAL_RELATIVE

    print(
        f"sum of medians over {len(shared)} groups measured on both sides: "
        f"{total_before:,.1f} -> {total_after:,.1f} ms ({total_rise:+.1%})"
        + (f"  REGRESSION: at least {TOTAL_RELATIVE:.0%}" if total_regressed else "")
    )

    print(
        f"\ngroups slowed past the threshold (median up {GROUP_RELATIVE:.0%} and {GROUP_ABSOLUTE_MS:.0f} ms, "
        f"runs not overlapping): {len(slowed)}"
    )
    if slowed:
        print(f"    {'conv':>6} {'baseline':>18} {'run':>18} {'change':>9}")
        for conv in sorted(slowed, key=lambda conv: median_ms(after[conv]) - median_ms(before[conv]), reverse=True):
            was, now = before[conv], after[conv]
            print(f"    {conv:>6} {range_of(was):>18} {range_of(now):>18} {median_ms(now) / median_ms(was) - 1:>+9.0%}")

    print(f"\ngroups the baseline measured and the run did not: {len(lost)}")
    for conv in lost:
        print(f"    {conv}: {before[conv]['menu_ms_median']} ms -> {after[conv]['menu_ms_median']}")

    regressed = bool(slowed or lost or total_regressed)
    print(f"\n{'REGRESSION FLAGGED' if regressed else 'no regression flagged'}")
    return regressed


def range_of(row):
    """A group's median and its runs' range, as `median (min-max)`."""
    return f"{median_ms(row):,.0f} ({row['menu_ms_min']}-{row['menu_ms_max']})"


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--baselines", default=str(BASELINES), help="where marked baselines are kept")
    commands = parser.add_subparsers(dest="command", required=True)

    marking = commands.add_parser("mark", help="copy a several-run measurement into the baselines")
    marking.add_argument("folder", help="the measure_menus folder to mark")
    marking.add_argument("--note", default="", help="why this run is a baseline")

    commands.add_parser("list", help="list the marked baselines, newest first")

    checking = commands.add_parser("check", help="flag what regressed in a run against its baseline")
    checking.add_argument("folder", help="the measure_menus folder to check")
    checking.add_argument("--baseline", help="the baseline to hold it to, by name, instead of the newest match")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        if args.command == "mark":
            mark(args.folder, args.baselines, args.note)
        elif args.command == "list":
            list_baselines(args.baselines)
        elif check(args.folder, args.baselines, args.baseline):
            return EXIT_REGRESSED
    except Refused as refused:
        print(f"REFUSED: {refused}", file=sys.stderr)
        return EXIT_REFUSED
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
