#!/usr/bin/env python
# run-log-kind: analysis

"""Which entries' Sequence could hold the screen, and how many there are.

## The question

The offline walk decides whether a line waits for a continue from its LINK SHAPE: a line with
another line behind it waits, a line with a menu behind it does not. Measured in game that is
right almost everywhere and wrong on conversation 1467, where two entries hold the screen the
walk walks straight past.

What separates them is the dialogue entry's `Sequence`, which the shipped index does not carry.
A sequence that RUNS - an animation, a command scheduled with `@` - keeps its line on screen; a
fire-and-forget order does not. See de-oaaq.

## What it reports

The game's whole sequence vocabulary, then the population a corrected rule could move: entries
whose sequence is something other than a bare `Continue()` AND which have a menu behind them,
split by whether their commands plausibly hold.

THE SPLIT IS A NAMED LIST, NOT A GUESS. `HOLDS` is what the two measured cases used plus their
obvious kin; `PASSES` is what was measured NOT to hold, which is the more important half - it is
why "carries a Sequence" is the wrong predicate. Anything on neither list is reported as unknown
rather than assumed, because assuming either way is how a rule gets a reputation it has not
earned.

## What it reads

`.game_reference_copies/derived/conversation_index.jsonl`, which `tools/DialogueExtract`'s
`conversation-index` writes - the full record, before `shipped-index` strips it to what the
engine reads.

Usage:
    tools/sequence-holds.py [--index PATH] [--list N]
"""

import argparse
import collections
import json
import re
import sys
import traceback

from pathlib import Path

###############################################################################
# Core functions
###############################################################################

INDEX = Path(".game_reference_copies/derived/conversation_index.jsonl")

# The actor every dialogue option in the sample is spoken by, which is how a menu is recognised.
OPTION_ACTOR = "396"

COMMAND = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)\s*\(")
# A command scheduled later than now, which gives a sequence a duration by construction.
TIMED = re.compile(r"@[\d.]+")

# Commands that plausibly hold the line. PlayAnimation is one of the two measured cases; the
# screen-covering ones are here because a line cannot be dismissed behind a blackout.
HOLDS = frozenset(
    {
        "PlayAnimation",
        "SetTriggerAnimation",
        "FadeToBlack",
        "TotalBlack",
        "SemiBlack",
        "PostFX",
        "BanishInterface",
    }
)

# Measured NOT to hold the screen, listed by name so they stay out of the count deliberately:
# 379:881 used LuaRun and SetAreaState and 656:27 used TravelTo, and in both the menu composed
# beside the line with no press.
PASSES = frozenset({"LuaRun", "SetAreaState", "TravelTo", "AudioPlay", "MusicPlay"})

# The sequence that says nothing beyond "wait", which almost every sequence in the game is.
PLAIN = "Continue"


def entries_of(row):
    return {entry["id"]: entry for entry in row.get("entries", ())}


def behind(entries, start):
    """What the player meets after `start`: 'menu', 'line' or 'nothing'.

    Group entries are walked through, since the game expands them in place and displays nothing
    for them - the same treatment the walk gives them. Links are followed and guards ignored,
    which over-approximates, so an entry called 'menu' here might reach one only sometimes.
    """
    seen = set()
    queue = list(entries.get(start, {}).get("to", ()))
    found_line = False
    while queue:
        node = queue.pop(0)
        if node in seen:
            continue
        seen.add(node)
        entry = entries.get(node)
        if entry is None:
            continue
        if entry.get("group"):
            queue.extend(entry.get("to", ()))
            continue
        if entry.get("fields", {}).get("Actor") == OPTION_ACTOR:
            return "menu"
        found_line = True
    return "line" if found_line else "nothing"


def verdict(sequence):
    """Whether a sequence holds the line: 'holds', 'passes' or 'unknown'."""
    names = set(COMMAND.findall(sequence))
    if TIMED.search(sequence) or (names & HOLDS):
        return "holds"
    if names <= (PASSES | {PLAIN}):
        return "passes"
    return "unknown"


def census(rows):
    """The whole game's sequences, and the population with a menu behind them."""
    counted = {"entries": 0, "sequenced": 0, "plain": 0, "timed": 0}
    before_menu = collections.defaultdict(list)
    unknown = collections.Counter()

    for row in rows:
        entries = entries_of(row)
        for entry in entries.values():
            counted["entries"] += 1
            sequence = entry.get("fields", {}).get("Sequence") or ""
            if not sequence:
                continue
            counted["sequenced"] += 1
            names = set(COMMAND.findall(sequence))
            if names == {PLAIN}:
                counted["plain"] += 1
            if TIMED.search(sequence):
                counted["timed"] += 1
            if entry.get("group") or names == {PLAIN} or not names:
                continue
            if behind(entries, entry["id"]) != "menu":
                continue
            said = verdict(sequence)
            before_menu[said].append((row["id"], entry["id"], sequence))
            if said == "unknown":
                unknown.update(names - PASSES - {PLAIN})

    return counted, before_menu, unknown


def report(path, listed):
    rows = (json.loads(line) for line in Path(path).open(encoding="utf-8"))
    counted, before_menu, unknown = census(rows)

    total = counted["entries"]
    sequenced = counted["sequenced"]
    print(f"{total:,} entries in the game")
    print(f"{sequenced:,} carry a Sequence ({sequenced / max(total, 1):.1%})")
    print(f"{counted['plain']:,} of those are {PLAIN}() alone ({counted['plain'] / max(sequenced, 1):.1%})")
    print(f"{counted['timed']:,} schedule something with @\n")

    print("with a menu behind them, and a sequence beyond Continue():")
    for said in ("holds", "passes", "unknown"):
        print(f"  {said:>8}  {len(before_menu[said]):>5,}")
    print()

    for conversation, entry, sequence in before_menu["holds"][:listed]:
        print(f"  {conversation}:{entry:<6} {sequence[:96]}")

    if unknown:
        print("\ncommands on neither list, by uses in this population:")
        for name, count in unknown.most_common(20):
            print(f"  {name:<28} {count:>5}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--index", default=str(INDEX), help="the full conversation index")
    parser.add_argument("--list", type=int, default=25, help="how many holders to name")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        report(args.index, args.list)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
