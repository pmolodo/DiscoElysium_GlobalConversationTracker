#!/usr/bin/env python

"""What each profile costs one engine, across a matrix run's TSVs.

Reads every performance-matrix-*.tsv in a run folder and reports, per profile, the
distribution of one engine's ms column plus how the row ended. It exists to answer whether
a profile is worth its place in the default grid: a profile that never costs a player
anything measurable is a tenth of every whole-game run spent confirming that it still does
not.

COLUMNS ARE MATCHED BY NAME, from each file's own header, because the matrix's columns have
moved several times. Reading them by position across two runs compares unrelated numbers and
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

# A row that never ran is not a fast row, and averaging it in as one is how a profile gets
# dropped for being cheap when it was actually absent.
NOT_A_MEASUREMENT = {"NOT-MEASURED", "CRASHED", "?", ""}

# What "basically always instant" is measured against.
INSTANT_MS = 1000.0


def profile_cost(folder, engine="ingame", instant_ms=INSTANT_MS):
    rows = list(read_rows(folder, engine))
    if not rows:
        print(f"no {engine} rows in {folder}", file=sys.stderr)
        return

    by_profile = {}
    for profile, ms, verdict in rows:
        by_profile.setdefault(profile, []).append((ms, verdict))

    print(f"{folder}\n{len(rows)} rows, engine {engine}, instant is under {instant_ms:.0f} ms\n")
    print(
        f"{'profile':>18}  {'rows':>5}  {'instant':>9}  {'median':>8}  {'p90':>8}  "
        f"{'p99':>8}  {'max':>9}  {'gated':>6}  {'not-run':>7}"
    )

    # Worst first, so the profiles that earn their place are at the top and the candidates
    # for dropping fall to the bottom.
    order = sorted(by_profile, key=lambda p: -percentile([m for m, _ in by_profile[p]], 99))
    for profile in order:
        measured = [ms for ms, verdict in by_profile[profile] if verdict not in NOT_A_MEASUREMENT]
        not_run = len(by_profile[profile]) - len(measured)
        gated = sum(1 for _, verdict in by_profile[profile] if verdict == "gated")
        if not measured:
            continue
        under = sum(1 for ms in measured if ms < instant_ms)
        print(
            f"{profile:>18}  {len(measured):>5}  {under / len(measured) * 100:>8.2f}%  "
            f"{percentile(measured, 50):>8.0f}  {percentile(measured, 90):>8.0f}  "
            f"{percentile(measured, 99):>8.0f}  {max(measured):>9.0f}  {gated:>6}  "
            f"{not_run:>7}"
        )

    print(
        "\nINSTANT is the share of measured rows under the threshold. Read it with p99 and "
        "max:\na profile that is instant on 99 per cent of groups and spends the cap on the "
        "rest is the\ntail the matrix exists to find, not a profile to drop. GATED rows are "
        "ones the game would\nnot have searched at all, and NOT-RUN rows are crashes and "
        "unsupplied budgets - neither is\na fast row and neither is counted in the "
        "distribution."
    )


def read_rows(folder, engine):
    """Every (profile, ms, verdict) the folder holds for one engine."""
    for path in sorted(glob.glob(os.path.join(folder, "performance-matrix-*.tsv"))):
        with open(path) as handle:
            header = handle.readline().rstrip("\n").split("\t")
            wanted = {f"{engine}_ms", f"{engine}_verdict", "profile"}
            if not wanted <= set(header):
                continue
            at = {name: header.index(name) for name in wanted}
            for line in handle:
                parts = line.rstrip("\n").split("\t")
                if len(parts) < len(header):
                    continue
                verdict = parts[at[f"{engine}_verdict"]]
                try:
                    ms = float(parts[at[f"{engine}_ms"]])
                except ValueError:
                    # A row that never ran carries "?" here, and `verdict` says which kind.
                    ms = 0.0
                yield parts[at["profile"]], ms, verdict


def percentile(values, which):
    """The `which`th percentile, nearest rank, or zero over nothing."""
    if not values:
        return 0.0
    ordered = sorted(values)
    rank = max(1, min(len(ordered), round(which / 100 * len(ordered))))
    return ordered[rank - 1]


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("folder", help="a matrix run folder under measurements/logs")
    parser.add_argument("--engine", default="ingame", help="which engine's columns to read")
    parser.add_argument(
        "--instant-ms",
        default=INSTANT_MS,
        type=float,
        help="what counts as costing a player nothing",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        profile_cost(args.folder, engine=args.engine, instant_ms=args.instant_ms)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
