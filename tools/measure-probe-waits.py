#!/usr/bin/env python

"""Total what a harness run spent waiting on the probe, grouped by what it waited for.

Reads a GameHarness log - the one it names on its second line, under testing/logs - and
adds up every "saw ... after Ns" line. Each of those is one wait: the harness asked for
something and sat until the probe said it had happened. The point of the grouping is that
a wait is rounded up to the watcher's poll interval, so an event whose mean sits on a
multiple of that interval is one the harness spent its time NOTICING rather than waiting
for.
"""

import argparse
import collections
import re
import sys
import traceback

###############################################################################
# Core functions
###############################################################################

# "        saw a 'load-finished' event after 3.1s"
WAIT = re.compile(r"^\s*saw (?P<what>.+?) after (?P<seconds>[0-9.]+)s\s*$")

# What a wait is called, reduced to what it is FOR. A menu names its conversation and a
# command its arguments, and neither is a different kind of wait - grouping by the whole
# phrase would make one row per scenario and say nothing.
GROUPS = [
    (re.compile(r"^a '(?P<name>[a-z-]+)' event$"), r"\g<name>"),
    (re.compile(r"^an answer to \S+.*$"), "an answer to a command"),
    (re.compile(r"^a response menu in conversation \d+$"), "a response menu"),
]


def group_of(what):
    for pattern, name in GROUPS:
        match = pattern.match(what)
        if match:
            return match.expand(name) if "\\g" in name else name
    return what


def waits_by_group(lines):
    """Maps each group to the list of seconds its waits took."""
    found = collections.defaultdict(list)
    for line in lines:
        match = WAIT.match(line)
        if match:
            found[group_of(match.group("what"))].append(float(match.group("seconds")))
    return found


def report(log_path, top=None):
    with open(log_path, encoding="utf-8", errors="replace") as handle:
        found = waits_by_group(handle)

    if not found:
        raise ValueError(f"{log_path} holds no 'saw ... after Ns' lines.")

    rows = sorted(found.items(), key=lambda row: -sum(row[1]))
    kept = rows if top is None else rows[:top]
    rest = rows[len(kept) :]

    print(f"{'event':<34}{'times':>6}{'total':>10}{'mean':>8}")
    for name, seconds in kept:
        total = sum(seconds)
        print(f"{name:<34}{len(seconds):>6}{total:>9.1f}s{total / len(seconds):>7.1f}s")

    if rest:
        total = sum(sum(seconds) for _, seconds in rest)
        times = sum(len(seconds) for _, seconds in rest)
        print(f"{'the rest':<34}{times:>6}{total:>9.1f}s")

    every = [one for _, seconds in rows for one in seconds]
    print(f"\n{len(every)} waits, {sum(every):.1f}s in all.")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("log", help="A GameHarness log, from testing/logs.")
    parser.add_argument(
        "--top",
        default=5,
        type=int,
        help="How many groups to name before collapsing the rest into one row.",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        report(args.log, top=args.top)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
