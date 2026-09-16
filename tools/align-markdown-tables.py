#!/usr/bin/env python

"""Pad every table in a markdown file so its columns line up in the raw text.

Each cell is padded to its column's widest cell. A column whose separator is right-aligned
(`---:`) is padded on the left, so numbers line up by their last digit; every other column is
padded on the right. Tables inside fenced code blocks are left alone. The file is rewritten in
place, byte for byte except for the tables.
"""

import argparse
import re
import sys
import traceback

SEPARATOR_CELL = re.compile(r"^:?-+:?$")
FENCE = re.compile(r"^\s*(```|~~~)")

###############################################################################
# Core functions
###############################################################################


def split_row(line):
    """The cells of a table row, without the outer pipes. Escaped pipes stay in their cell."""
    body = line.strip()
    if body.startswith("|"):
        body = body[1:]
    if body.endswith("|") and not body.endswith("\\|"):
        body = body[:-1]
    return [cell.strip() for cell in re.split(r"(?<!\\)\|", body)]


def is_row(line):
    return line.lstrip().startswith("|")


def align_table(lines):
    rows = [split_row(line) for line in lines]
    width = max(len(row) for row in rows)
    rows = [row + [""] * (width - len(row)) for row in rows]
    separator = rows[1]
    if not all(SEPARATOR_CELL.match(cell) for cell in separator if cell):
        raise ValueError(f"not a table - second row is not a separator: {lines[1]!r}")
    right = [cell.endswith(":") and not cell.startswith(":") for cell in separator]
    widths = [max(len(row[col]) for i, row in enumerate(rows) if i != 1) for col in range(width)]
    widths = [max(w, 3) for w in widths]

    out = []
    for i, row in enumerate(rows):
        cells = []
        for col, cell in enumerate(row):
            if i == 1:
                cells.append("-" * (widths[col] - 1) + ":" if right[col] else "-" * widths[col])
            elif right[col]:
                cells.append(cell.rjust(widths[col]))
            else:
                cells.append(cell.ljust(widths[col]))
        out.append("| " + " | ".join(cells) + " |")
    return out


def align_text(text):
    lines = text.split("\n")
    out = []
    in_fence = False
    i = 0
    while i < len(lines):
        line = lines[i]
        if FENCE.match(line):
            in_fence = not in_fence
        if in_fence or not is_row(line):
            out.append(line)
            i += 1
            continue
        start = i
        while i < len(lines) and is_row(lines[i]):
            i += 1
        block = lines[start:i]
        out.extend(align_table(block) if len(block) >= 2 else block)
    return "\n".join(out)


def align_file(path, check):
    with open(path, encoding="utf-8", newline="") as handle:
        text = handle.read()
    aligned = align_text(text)
    if aligned == text:
        return False
    if check:
        print(f"{path}: tables are not aligned")
    else:
        with open(path, "w", encoding="utf-8", newline="") as handle:
            handle.write(aligned)
        print(f"{path}: aligned")
    return True


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("paths", nargs="+", help="markdown files to align")
    parser.add_argument("--check", action="store_true", help="report unaligned files and exit 2, writing nothing")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        changed = [path for path in args.paths if align_file(path, args.check)]
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 2 if args.check and changed else 0


if __name__ == "__main__":
    sys.exit(main())
