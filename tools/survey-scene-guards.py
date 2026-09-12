#!/usr/bin/env python

"""Which conversations guard on the scene queries, and what the guards say.

The question de-kxam turns on: these are only worth modelling if the game actually asks
them, and only worth a scenario if some entry's visibility really turns on one.
"""

import argparse
import json
import sys

CALLS = ("IsExterior", "IsRaining", "IsSnowing")


def scan(path, calls):
    found = []
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            if not any(call in line for call in calls):
                continue
            record = json.loads(line)
            conversation = record.get("id", record.get("conversation_id", "?"))
            title = record.get("title", "")
            for entry in record.get("entries", []):
                guard = entry.get("conditions") or entry.get("guard") or ""
                if any(call in guard for call in calls):
                    found.append(
                        {
                            "conversation": conversation,
                            "title": title,
                            "entry": entry.get("id", entry.get("entry_id", "?")),
                            "guard": " ".join(guard.split()),
                            "text": " ".join((entry.get("text") or "").split())[:90],
                        }
                    )
    return found


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("index")
    parser.add_argument("--calls", nargs="*", default=list(CALLS))
    args = parser.parse_args(argv)

    found = scan(args.index, args.calls)
    print(f"{len(found)} guarded entr(ies)\n")
    for row in found:
        print(f"conversation {row['conversation']} ({row['title']}) entry {row['entry']}")
        print(f"  guard: {row['guard']}")
        if row["text"]:
            print(f"  text:  {row['text']}")
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
