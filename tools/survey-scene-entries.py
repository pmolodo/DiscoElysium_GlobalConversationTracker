#!/usr/bin/env python

"""What the scene-guarded entries of one conversation are, and who reaches them.

`survey-scene-guards.py` says WHICH conversations guard on the scene. This says what the
guarded entries are inside one of them, and that is the question a scenario turns on: the
guards sit on GROUP entries rather than on player options, so what an answer changes is
which subtree a crawl walks, not which option a menu draws.

Over conversation 29 it prints the chain the scene routes through - 226 to either 227 or
228, and the weather only ever asked below 227, which is the outdoor side.
"""

import argparse
import json
import os
import sys

INDEX = ".game_reference_copies/derived/conversation_index.trimmed.jsonl"
QUERIES = ("IsExterior", "IsRaining", "IsSnowing")


def row_for(path, conversation):
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            if row.get("id") == conversation:
                return row
    raise SystemExit(f"conversation {conversation} is not in {path}")


def report(repo, conversation):
    row = row_for(os.path.join(repo, INDEX), conversation)
    entries = {entry["id"]: entry for entry in row["entries"]}

    guarded = [entry for entry in row["entries"] if any(call in (entry.get("guard") or "") for call in QUERIES)]

    print(f"conversation {conversation}: {len(entries)} entries, {len(guarded)} scene-guarded\n")

    reaches = {}
    for entry in row["entries"]:
        for to in entry.get("to", []):
            reaches.setdefault(to, []).append(entry["id"])

    for entry in guarded:
        eid = entry["id"]
        print(f"  entry {eid}")
        print(f"    guard  : {entry.get('guard')}")
        print(f"    group  : {entry.get('group')}")
        print(f"    title  : {entry.get('title')}")
        print(f"    to     : {entry.get('to')}")
        print(f"    from   : {reaches.get(eid, [])}")
        for parent in reaches.get(eid, []):
            held = entries.get(parent, {})
            print(f"      {parent}: group={held.get('group')} guard={held.get('guard')!r}")
        print()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=os.getcwd())
    parser.add_argument("--conversation", type=int, default=29)
    args = parser.parse_args(argv)
    report(args.repo, args.conversation)
    return 0


if __name__ == "__main__":
    sys.exit(main())
