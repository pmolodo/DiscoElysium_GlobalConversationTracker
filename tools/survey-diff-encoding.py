#!/usr/bin/env python

"""What the committed sparse files look like, byte for byte.

Answers the question de-xz48.6.1 turns on: can a Rust writer reproduce these exactly, and
which formatting rules does it have to match? Reports the indent, the separators, the
trailing bytes, the number spellings and any escape that is not forced.
"""

import argparse
import collections
import glob
import json
import re
import sys

###############################################################################
# Core functions
###############################################################################


def survey(paths):
    indents = collections.Counter()
    tails = collections.Counter()
    separators = collections.Counter()
    numbers = collections.Counter()
    escapes = collections.Counter()
    non_ascii = 0

    for path in paths:
        with open(path, "rb") as handle:
            raw = handle.read()
        text = raw.decode("utf-8")

        tails[repr(text[-3:])] += 1

        # The indent, from the first nested line.
        for line in text.split("\n")[1:]:
            stripped = line.lstrip(" ")
            if stripped and stripped != line:
                indents[len(line) - len(stripped)] += 1
                break

        if '": ' in text:
            separators['": "'] += 1
        if '":' in text.replace('": ', ""):
            separators['":" (no space)'] += 1

        for found in re.findall(r":\s*(-?\d+\.?\d*(?:[eE][-+]?\d+)?)", text):
            if "." in found or "e" in found.lower():
                numbers["fractional"] += 1
            else:
                numbers["integer"] += 1

        for found in re.findall(r"\\(.)", text):
            escapes[found] += 1

        if any(ord(char) > 127 for char in text):
            non_ascii += 1

        # And that it parses, so the survey is of real documents.
        json.loads(text)

    print(f"{len(paths)} file(s)")
    print(f"  indent widths:  {dict(indents)}")
    print(f"  last 3 chars:   {dict(tails)}")
    print(f"  separators:     {dict(separators)}")
    print(f"  numbers:        {dict(numbers)}")
    print(f"  escapes seen:   {dict(escapes)}")
    print(f"  files with non-ASCII text: {non_ascii}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "--pattern",
        default="testing/scenarios/**/*.json",
        help="Which committed files to survey.",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        paths = [path for path in glob.glob(args.pattern, recursive=True) if "_archive" not in path]
        survey(paths)
    except Exception:  # pylint: disable=broad-except
        import traceback

        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
