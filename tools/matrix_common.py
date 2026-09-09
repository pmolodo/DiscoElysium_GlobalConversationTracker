#!/usr/bin/env python

"""Reading a matrix folder, and splitting it into the three profiles every comparison wants.

## Why a total is not an answer

A whole-game matrix is 521 groups and their costs are nothing like each other: conversation
761 takes two seconds a menu and most groups take under one millisecond. So a total is very
nearly the sum of the heaviest few, and it hides both of the changes worth knowing about - one
that helps the heavy groups and hurts five hundred small ones reads as a straight win, and one
that helps five hundred small groups by a little reads as no change at all.

de-12wr.10. Every whole-game comparison reports EVERYTHING, THE HARDEST FEW and THE EASIEST
HUNDRED, so neither of those can hide.

## Ranked by the census, so the buckets do not move

The obvious ranking is by the measured time in the run being read, and it is wrong: the two
arms of a comparison would then be split by different rankings, so a group could be hard in
one arm and easy in the other and the buckets would compare different populations.

The census is taken ONCE and shared by both arms - the matrix driver puts it in the run folder
and a comparison reads one of them - so ranking by it splits both arms identically. Its `ms`
column is the time its own reachability pass took over the group, which tracks search
difficulty closely: it puts 761 first, and 368, 14 and 1030 in its top eight, which is what the
matrix says too.

`candidates` would be the more obviously stable choice and is the worse one. It ranks 631, 640
and 16 first, none of which is among the slowest five, because how big a group is says little
about how expensive it is - 368 and 631 are within five per cent of each other in size and
differ by thirty-five times in cost.
"""

import glob
import os

TAB = "\t"

# A row that never ran is not a measurement, and pairing one with a real row would report a
# speed-up or a slow-down that never happened.
NOT_A_MEASUREMENT = {"NOT-MEASURED", "CRASHED", "?", "", "NO-ROWS"}

# How many groups each profile holds. Five is small enough that the groups can be named one by
# one in a report; a hundred is enough that a per-group difference of a millisecond adds up to
# something a reader can see.
HARDEST = 5
EASIEST = 100


def rows_of(folder, engine=None):
    """Every (conv, profile) -> {column: value} the folder holds.

    With `engine`, the columns are that engine's with its prefix stripped, so a caller reads
    `row["ms"]` whatever the engine is called. Without one, the row is every column under its
    own name.

    THE LAST ROW PER KEY WINS, which is the rule the whole matrix reads by: the files are
    appended to, so a retried row sits after the one it replaces.

    COLUMNS ARE MATCHED BY NAME from each file's own header, never by position. The matrix's
    columns have moved several times, and reading them positionally across two runs compares
    unrelated numbers and produces a plausible table.
    """
    rows = {}
    for path in sorted(glob.glob(os.path.join(folder, "performance-matrix-*.tsv"))):
        with open(path, encoding="utf-8", errors="replace") as handle:
            header = handle.readline().rstrip("\n").replace("\r", "").split(TAB)
            if "conv" not in header or "profile" not in header:
                continue
            if engine is None:
                wanted = {name: index for index, name in enumerate(header)}
            else:
                if f"{engine}_verdict" not in header:
                    continue
                wanted = {
                    name[len(engine) + 1 :]: index for index, name in enumerate(header) if name.startswith(f"{engine}_")
                }
            at_conv, at_profile = header.index("conv"), header.index("profile")
            for line in handle:
                cells = line.rstrip("\n").replace("\r", "").split(TAB)
                if len(cells) < len(header):
                    continue
                key = (cells[at_conv], cells[at_profile])
                rows[key] = {name: cells[index] for name, index in wanted.items()}
    return rows


def census_of(folder):
    """conv -> {column: value} from the census the run folder carries, or `{}` if it has none.

    A run without one is not an error here - a comparison can still report EVERYTHING, and
    `ranked` says so rather than inventing an order.
    """
    path = os.path.join(folder, "census", "census.tsv")
    if not os.path.exists(path):
        return {}
    out = {}
    with open(path, encoding="utf-8", errors="replace") as handle:
        header = handle.readline().rstrip("\n").replace("\r", "").split(TAB)
        if "conv" not in header:
            return {}
        at_conv = header.index("conv")
        for line in handle:
            cells = line.rstrip("\n").replace("\r", "").split(TAB)
            if len(cells) < len(header):
                continue
            out[cells[at_conv]] = dict(zip(header, cells))
    return out


def ranked(census, by="ms"):
    """The conversations the census covers, hardest first, or `None` where it cannot say."""
    if not census:
        return None

    def cost(conv):
        cell = census[conv].get(by, "")
        try:
            return float(cell)
        except ValueError:
            return 0.0

    return sorted(census, key=cost, reverse=True)


def profiles(census, hardest=HARDEST, easiest=EASIEST):
    """The three readings, as (name, conversations or None).

    `None` for the conversations of EVERYTHING means "no filter", which is different from an
    empty set and has to stay different: a caller that treated them alike would report nothing
    at all for the reading that matters most.

    A run with no census gets EVERYTHING and nothing else, and says why.
    """
    order = ranked(census)
    if order is None:
        return [("everything", None)]
    return [
        ("everything", None),
        (f"hardest {hardest}", set(order[:hardest])),
        (f"easiest {easiest}", set(order[-easiest:])),
    ]


def totalled(rows, keys, column, among=None):
    """What one column sums to over `keys`, skipping rows neither arm measured.

    `among` is a set of conversation ids, or `None` for all of them.
    """
    total = 0.0
    counted = 0
    for key in keys:
        if among is not None and key[0] not in among:
            continue
        row = rows.get(key, {})
        if row.get("verdict", "") in NOT_A_MEASUREMENT or row.get("verdict", "").startswith("SKIPPED"):
            continue
        try:
            total += float(row[column])
        except (KeyError, ValueError):
            continue
        counted += 1
    return total, counted


def report(before, after, keys, column, census, noise=0.10):
    """One column, read three ways, before against after.

    The percentage is what a reader acts on: a total that moved by less than the noise on a
    machine's wall clock is not a finding, and this says which of the three did.
    """
    print(f"\n{column} BY PROFILE")
    print(f"{'':>16}{'groups':>9}{'rows':>8}{'before':>14}{'after':>14}{'change':>10}")
    for name, among in profiles(census):
        was, rows_counted = totalled(before, keys, column, among)
        now, _ = totalled(after, keys, column, among)
        groups = len({key[0] for key in keys if among is None or key[0] in among})
        if not was:
            print(f"{name:>16}{groups:>9}{rows_counted:>8}{was:>14.0f}{now:>14.0f}{'n/a':>10}")
            continue
        share = (now - was) / was
        flag = "" if abs(share) < noise else "  <-"
        print(f"{name:>16}{groups:>9}{rows_counted:>8}{was:>14.0f}{now:>14.0f}{share * 100:>9.1f}%{flag}")
    if not census:
        print("  only EVERYTHING: this run folder carries no census to rank the groups by.")
