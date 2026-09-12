#!/usr/bin/env python

"""How much of a run string a one-entry change rewrites, over the committed corpus."""

import json
import os
import subprocess
import sys

REPO = r"D:/Downloads/Apps/Games/Disco Elysium/DiscoElysium_GlobalConversationTracker"
HOST = os.path.join(REPO, "target", "release", "gct-engine-host.exe")


def ids(text):
    found = set()
    for part in text.split(","):
        if not part:
            continue
        lo, _, hi = part.partition("-")
        found |= set(range(int(lo), int(hi) + 1)) if hi else {int(lo)}
    return found


def dump(save):
    out = subprocess.run(
        [HOST, "dump", save, "Conversation"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        cwd=REPO,
    )
    if out.returncode != 0:
        raise SystemExit(out.stderr)
    return json.loads(out.stdout)


def main():
    base, variant = sys.argv[1], sys.argv[2]
    a, b = dump(base), dump(variant)

    total_written = 0
    total_moved = 0
    for conversation in sorted(set(a) | set(b), key=str):
        row_a, row_b = a.get(conversation), b.get(conversation)
        if not isinstance(row_a, dict) or not isinstance(row_b, dict):
            continue
        held_a = row_a.get("Dialog", {})
        held_b = row_b.get("Dialog", {})
        if not isinstance(held_a, dict) or not isinstance(held_b, dict):
            continue

        for status in sorted(set(held_a) | set(held_b)):
            was, now = held_a.get(status, ""), held_b.get(status, "")
            if not isinstance(was, str) or not isinstance(now, str) or was == now:
                continue

            moved = ids(was) ^ ids(now)
            total_written += len(now)
            total_moved += len(moved)
            print(f"{conversation}/{status}: {len(moved)} ids differ, and {len(now)} characters are rewritten")

    print(f"\n{total_moved} ids moved, {total_written} characters written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
