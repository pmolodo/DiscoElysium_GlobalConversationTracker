#!/usr/bin/env python

"""Classify every run-looking string in the committed sparse tables by its order.

Answers whether a "descending" or an "unordered" case exists at all, and whether the
ones that do ever CHANGE between a save and the save it is a diff of.
"""

import argparse
import json
import re
import sys
import traceback

from collections import Counter
from pathlib import Path

###############################################################################
# Core functions
###############################################################################

GROUP = re.compile(r"^-?\d+(--?\d+)?$")

ASCENDING = "ascending"
DESCENDING = "descending"
UNORDERED = "unordered"


def unpack(text):
    """The ids a run holds, or None where the string is not a run."""
    if not text:
        return None
    ids = []
    for part in text.split(","):
        if not GROUP.match(part):
            return None
        at = part.find("-", 1)
        if at == -1:
            ids.append(int(part))
            continue
        first, last = int(part[:at]), int(part[at + 1 :])
        step = 1 if last >= first else -1
        ids.extend(range(first, last + step, step))
    return ids


def pack(ids):
    """The same spelling the Rust encoder uses."""
    text = []
    at = 0
    while at < len(ids):
        last = at
        if at + 1 < len(ids) and ids[at + 1] - ids[at] in (1, -1):
            step = ids[at + 1] - ids[at]
            while last + 1 < len(ids) and ids[last + 1] - ids[last] == step:
                last += 1
        text.append(str(ids[at]) if last == at else f"{ids[at]}-{ids[last]}")
        at = last + 1
    return ",".join(text)


def order_of(text):
    """Which of the three orders a run is in, or None where it is not a run."""
    ids = unpack(text)
    if ids is None or pack(ids) != text:
        return None
    if all(a < b for a, b in zip(ids, ids[1:])):
        return ASCENDING
    if all(a > b for a, b in zip(ids, ids[1:])):
        return DESCENDING
    return UNORDERED


def walk(node, path, found):
    if isinstance(node, dict):
        for key, value in node.items():
            walk(value, f"{path}/{key}", found)
    elif isinstance(node, str):
        order = order_of(node)
        if order is not None:
            found.append((order, path, node))


def survey(root):
    counts = Counter()
    interesting = []
    for name in sorted(Path(root).rglob("*.json")):
        found = []
        walk(json.loads(name.read_text(encoding="utf-8")), "", found)
        for order, path, text in found:
            counts[order] += 1
            if order != ASCENDING:
                interesting.append((order, name, path, text))

    for order in (ASCENDING, DESCENDING, UNORDERED):
        print(f"{order:>10}  {counts[order]}")
    print()
    for order, name, path, text in interesting:
        shown = text if len(text) <= 70 else text[:67] + "..."
        print(f"{order:>10}  {name}\n            {path} = {shown}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("root", help="A directory of committed sparse tables")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        survey(args.root)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
