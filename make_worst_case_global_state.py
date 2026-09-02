#!/usr/bin/env -S uv run --script

# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

"""Write the global state that makes a look-ahead crawl as expensive as it can be.

Every dialogue entry of every conversation is recorded as WasDisplayed. That is what
makes it the worst case, and both halves matter:

  - No option's own novelty is unseen-anywhere, so MarkerFor cannot answer from the
    option alone and return before it builds a graph. The crawl runs.
  - Nothing the crawl reaches is unseen-anywhere either, so it can never stop the
    instant it finds something. It has to explore everything reachable.

Recording only the conversations a scenario opens is not enough, and fails quietly.
Options in a menu often belong to a different conversation than the one that is
active - opening 28 (WHIRLING F1 / GARTE MAIN) draws a menu whose options are
entries of 13 (WHIRLING F1 / GARTE) - and an option whose own conversation is
absent from the state is unseen-anywhere, takes the early exit, and is never
crawled at all. The measurement then reports nothing for that conversation while
looking like it ran.

The source is the conversation index extracted by extract_conversation_index.py.
Output matches what GlobalStateJson writes: format version 3, grouped by status,
conversation and entry ids ascending, no indentation, UTF-8 without a BOM - plus a
trailing newline, which the repository's end-of-file hook adds anyway and which
JSON ignores. Without it every regeneration would dirty the tree.
"""

import argparse
import json
import os
import sys
import traceback

###############################################################################
# Core functions
###############################################################################

# What the game calls an entry the player has been shown. Written as the game's own
# string rather than an enum integer, the same way GlobalStateJson writes it.
DISPLAYED = "WasDisplayed"

# The format version GlobalStateJson writes: grouped by status, then by conversation,
# then an array of entry ids. Orbs are left out, since the reader treats an absent
# "orbs" property as no orbs and a worst-case crawl does not look at them.
FORMAT_VERSION = 3

DEFAULT_INDEX = os.path.join(".game_reference_copies", "derived", "conversation_index.jsonl")
DEFAULT_OUTPUT = os.path.join("testing", "scenarios", "global-state-worst-case.json")


def read_index(index_path):
    """Yields (conversation id, sorted entry ids) for every conversation."""
    with open(index_path, encoding="utf-8") as handle:
        for number, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                conversation = json.loads(line)
            except json.JSONDecodeError as error:
                raise ValueError(f"{index_path}:{number} is not JSON: {error}") from error

            entries = sorted({entry["id"] for entry in conversation["entries"]})
            yield conversation["id"], entries


def build_state(index_path):
    """Builds the state object, with everything recorded as displayed."""
    conversations = {}
    for conversation_id, entries in read_index(index_path):
        if not entries:
            continue
        conversations[str(conversation_id)] = entries

    if not conversations:
        raise ValueError(f"{index_path} described no conversations at all.")

    ordered = {key: conversations[key] for key in sorted(conversations, key=int)}
    return {"version": FORMAT_VERSION, "conversations": {DISPLAYED: ordered}}


def write_state(index_path, output_path):
    """Writes the worst-case state, and reports what it holds."""
    state = build_state(index_path)
    text = json.dumps(state, separators=(",", ":"), ensure_ascii=False)

    with open(output_path, "w", encoding="utf-8", newline="") as handle:
        handle.write(text)
        handle.write("\n")

    displayed = state["conversations"][DISPLAYED]
    entries = sum(len(v) for v in displayed.values())
    print(
        f"{output_path}: {len(displayed):,} conversations, "
        f"{entries:,} entries, {len(text.encode('utf-8')) / 1_000_000:.2f} MB"
    )


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "--index",
        default=DEFAULT_INDEX,
        help="The conversation index from extract_conversation_index.py",
    )
    parser.add_argument(
        "--output",
        default=DEFAULT_OUTPUT,
        help="Where to write the global state",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        write_state(args.index, args.output)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
