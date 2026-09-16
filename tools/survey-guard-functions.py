#!/usr/bin/env python

"""Count the functions the shipped guard corpus calls.

For each function named in a guard, two numbers:

- `guards`: how many DISTINCT guards call it, from `distinct_guards.txt`. This is the number
  `docs/guards.md` quotes, because a guard text repeated on many entries is still one question
  to get right.
- `entries`: how many dialogue entries carry a guard calling it, from `conversation_index.jsonl`,
  which is the number that says how often the question is asked.

Comments (`--[[ ... ]]`) and string literals are blanked before scanning, so a function named
inside a string or a comment is not counted.
"""

import argparse
import collections
import json
import re
import sys
import traceback

DEFAULT_DERIVED = ".game_reference_copies/derived"
CALL = re.compile(r"(?<![\w.])([A-Za-z_]\w*)\s*\(")
LUA_KEYWORDS = {"and", "or", "not", "if", "then", "return", "function", "end"}
BLOCK_COMMENT = re.compile(r"--\[\[.*?\]\]", re.DOTALL)
STRING = re.compile(r'"(?:\\.|[^"\\])*"')

###############################################################################
# Core functions
###############################################################################


def called_functions(guard):
    """The distinct function names a guard calls."""
    code = STRING.sub('""', BLOCK_COMMENT.sub(" ", guard))
    return {name for name in CALL.findall(code) if name not in LUA_KEYWORDS}


def count_distinct(path):
    counts = collections.Counter()
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            counts.update(called_functions(line.rstrip("\n")))
    return counts


def count_entries(path):
    counts = collections.Counter()
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            for entry in json.loads(line)["entries"]:
                counts.update(called_functions(entry.get("guard") or ""))
    return counts


def survey(derived):
    guards = count_distinct(f"{derived}/distinct_guards.txt")
    entries = count_entries(f"{derived}/conversation_index.jsonl")
    names = sorted(set(guards) | set(entries), key=lambda n: (-guards[n], n))
    print(f"{'function':<30} {'guards':>7} {'entries':>8}")
    for name in names:
        print(f"{name:<30} {guards[name]:>7} {entries[name]:>8}")
    print(f"\n{len(names)} functions")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--derived",
        default=DEFAULT_DERIVED,
        help="the extractor's derived folder (default: %(default)s)",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        survey(args.derived)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
