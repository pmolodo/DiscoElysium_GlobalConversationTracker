#!/usr/bin/env python

"""Is the forward slice worth its place, and on which groups?

de-dt75.1. The matrix measures the switching method twice per row: `ingame` is what the game
runs - a forward slice, then the backward driver told what it found - and `bwd-ingame` is the
same method with the slice off. The two differ in ONE field, so a difference between the
columns is the slice and cannot be anything else.

## The verdicts have to agree before any of it is a performance question

A group where turning the slice off changes an ANSWER is not a group where the slice is
slower; it is a group where the two arms do not compute the same thing, and that has to be
understood before anything is decided. So the disagreements are reported first and separately,
and a run with any of them should stop there.

## What the timing columns can and cannot say

`ms` ON A SLICE-BEARING COLUMN IS MACHINE-DEPENDENT - de-12wr.3. The slice is given fifty
milliseconds and does as much as fifty milliseconds of that machine buys, so whether it
answers a row it is close to answering is a property of the machine. The same driver run four
times flipped one row's `by` from Forwards to Backwards and back.

That makes a per-row `ms` difference weak evidence and a per-GROUP total, summed over five
profiles, rather better - but it is still a ratio to read with the `by` counts beside it
rather than a number to trust alone. What is solid is `by`: how often the slice ANSWERED.

## Usage

    python tools/matrix-slice-worth.py measurements/logs/<a whole-game folder>
"""

import argparse
import sys

from pathlib import Path

TAB = "\t"

# The pair that differs in one field. `ingame` runs the slice; `bwd-ingame` is the same method
# with `portfolio::Budget::forwards` at zero.
WITH_SLICE = "ingame"
WITHOUT_SLICE = "bwd-ingame"


def rows_of(folder):
    """Every row in the folder, as dicts keyed by column name.

    THE LAST ROW PER (conv, profile) WINS, which is the rule the whole matrix reads by: the
    files are appended to, so a retried row sits after the one it replaces.
    """
    latest = {}
    for path in sorted(Path(folder).glob("performance-matrix-*.tsv")):
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        if not lines:
            continue
        header = lines[0].replace("\r", "").split(TAB)
        for line in lines[1:]:
            cells = line.replace("\r", "").split(TAB)
            if len(cells) != len(header):
                continue
            row = dict(zip(header, cells))
            if "conv" not in row or "profile" not in row:
                continue
            latest[(row["conv"], row["profile"])] = row
    return latest


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("folder", help="a matrix run folder")
    parser.add_argument(
        "--top",
        type=int,
        default=15,
        help="how many groups to name at each end of the ranking",
    )
    args = parser.parse_args(argv)

    rows = rows_of(args.folder)
    if not rows:
        print(f"no rows in {args.folder}", file=sys.stderr)
        return 1

    needed = [f"{WITH_SLICE}_verdict", f"{WITHOUT_SLICE}_verdict"]
    sample = next(iter(rows.values()))
    missing = [name for name in needed if name not in sample]
    if missing:
        print(f"{args.folder} has no {', '.join(missing)} column", file=sys.stderr)
        print("This needs a run measuring both in-game arms, which is the default grid.")
        return 1

    disagreements = []
    per_group = {}
    for (conv, profile), row in sorted(rows.items()):
        with_verdict = row[f"{WITH_SLICE}_verdict"]
        without_verdict = row[f"{WITHOUT_SLICE}_verdict"]

        # A ROW NEITHER ARM MEASURED IS NOT EVIDENCE. Skipped, crashed and not-measured rows
        # carry a rule or a reason in the verdict column rather than an answer.
        if with_verdict in ("CRASHED", "NOT-MEASURED", "NO-ROWS") or with_verdict.startswith("SKIPPED"):
            continue

        if with_verdict != without_verdict:
            disagreements.append((conv, profile, with_verdict, without_verdict))

        group = per_group.setdefault(conv, {"rows": 0, "with_ms": 0, "without_ms": 0, "answered_by_slice": 0})
        group["rows"] += 1
        for key, column in (("with_ms", WITH_SLICE), ("without_ms", WITHOUT_SLICE)):
            cell = row.get(f"{column}_ms", "")
            if cell.isdigit():
                group[key] += int(cell)
        if row.get(f"{WITH_SLICE}_by") == "Forwards":
            group["answered_by_slice"] += 1

    print(f"{args.folder}\n{len(rows)} rows, {len(per_group)} group(s) with a measured row\n")

    # FIRST, AND ON ITS OWN. Until these are zero nothing below is a performance question.
    print(f"VERDICTS THAT DISAGREE BETWEEN THE TWO ARMS: {len(disagreements)}")
    for conv, profile, one, other in disagreements[:20]:
        print(f"  {conv} {profile}: with the slice {one}, without it {other}")
    if not disagreements:
        print("  none - the two arms compute the same thing, so the rest is about cost.")
    print()

    total_with = sum(g["with_ms"] for g in per_group.values())
    total_without = sum(g["without_ms"] for g in per_group.values())
    answered = sum(g["answered_by_slice"] for g in per_group.values())
    measured = sum(g["rows"] for g in per_group.values())

    print(f"{'':>8}{'with slice':>14}{'without':>12}{'difference':>13}")
    print(f"{'total ms':>8}{total_with:>14}{total_without:>12}{total_with - total_without:>13}")
    print(f"\nthe slice ANSWERED {answered} of {measured} measured rows ({100.0 * answered / max(1, measured):.1f}%)")

    # WHERE THE SLICE PAYS AND WHERE IT DOES NOT, per group, since that is the unit the issue
    # asks the switch to be made on.
    ranked = sorted(
        per_group.items(),
        key=lambda item: item[1]["without_ms"] - item[1]["with_ms"],
        reverse=True,
    )
    print("\nGROUPS THE SLICE HELPS MOST (without - with, in ms over all profiles)\n")
    print(f"{'conv':>8}{'with':>10}{'without':>10}{'saved':>9}{'answered':>10}")
    for conv, group in ranked[: args.top]:
        print(
            f"{conv:>8}{group['with_ms']:>10}{group['without_ms']:>10}"
            f"{group['without_ms'] - group['with_ms']:>9}"
            f"{group['answered_by_slice']:>7}/{group['rows']}"
        )

    print("\nGROUPS THE SLICE COSTS MOST\n")
    print(f"{'conv':>8}{'with':>10}{'without':>10}{'cost':>9}{'answered':>10}")
    for conv, group in ranked[-args.top :][::-1]:
        print(
            f"{conv:>8}{group['with_ms']:>10}{group['without_ms']:>10}"
            f"{group['with_ms'] - group['without_ms']:>9}"
            f"{group['answered_by_slice']:>7}/{group['rows']}"
        )

    helps = sum(1 for _, g in ranked if g["without_ms"] > g["with_ms"])
    costs = sum(1 for _, g in ranked if g["with_ms"] > g["without_ms"])
    print(f"\nthe slice is faster on {helps} group(s), slower on {costs}, and level on {len(ranked) - helps - costs}.")

    # THE CUT THAT ACTUALLY DECIDES IT, and the one a per-group total hides: a row where the
    # slice ANSWERED and a row where it merely ran are two different populations, and the
    # slice is a bargain in the first and pure cost in the second. A group total mixes them,
    # so a switch decided on group totals is deciding on the average of two things.
    buckets = {"answered": [0, 0, 0], "ran and did not answer": [0, 0, 0]}
    for row in rows.values():
        verdict = row.get(f"{WITH_SLICE}_verdict", "")
        if verdict in ("CRASHED", "NOT-MEASURED", "NO-ROWS") or verdict.startswith("SKIPPED"):
            continue
        which = "answered" if row.get(f"{WITH_SLICE}_by") == "Forwards" else "ran and did not answer"
        bucket = buckets[which]
        bucket[0] += 1
        for index, column in ((1, WITH_SLICE), (2, WITHOUT_SLICE)):
            cell = row.get(f"{column}_ms", "")
            if cell.isdigit():
                bucket[index] += int(cell)

    print("\nTHE SAME ROWS SPLIT BY WHETHER THE SLICE ANSWERED\n")
    print(f"{'':>24}{'rows':>7}{'with':>10}{'without':>10}{'difference':>12}")
    for which, (count, with_ms, without_ms) in buckets.items():
        print(f"{which:>24}{count:>7}{with_ms:>10}{without_ms:>10}{without_ms - with_ms:>12}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
