#!/usr/bin/env -S uv run --script

# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

"""Extract a compact index of every conversation from a Dialogue System database .asset.

The index is what makes it possible to choose an in-game test's conversation on the
shape of its graph - how many options a menu offers, how far a forward scan can
travel from one - rather than on whatever happens to stand near the player in a save.

One JSON object per conversation is written, one per line:

  {"id": 8, "title": "...", "actor": 12, "conversant": 12,
   "entries": [{"id": 3, "group": false, "guard": "Variable[...] == true",
                "script": "Money = Money - 50", "to": [4, 9],
                "fields": {"DifficultyPass": "12", "Title": "..."}}]}

The guard and script carry the conditionsString and userScript verbatim, because
what a forward scan can reach turns on them: a purchase is a guard that tests money
and a script that spends it, and choosing a test scenario means reading both.

The .asset is a Unity-serialized YAML document of ~170 MB, so it is streamed line by
line at fixed indentation rather than parsed as YAML, the same way
extract_dialogue_corpus.py reads it.
"""

import argparse
import json
import os
import sys
import traceback

###############################################################################
# Core functions
###############################################################################

CONVERSATIONS = "  conversations:"
NEXT_SECTION = "  syncInfo:"

CONVERSATION_START = "  - id: "
CONVERSATION_FIELD = "    - title: "
CONVERSATION_VALUE = "      value: "

ENTRY_START = "    - id: "
ENTRY_FIELD = "      - title: "
ENTRY_VALUE = "        value: "
ENTRY_IS_GROUP = "      isGroup: "
ENTRY_CONDITIONS = "      conditionsString: "
ENTRY_SCRIPT = "      userScript: "
LINK_DESTINATION = "        destinationDialogueID: "
LINK_DESTINATION_CONVERSATION = "        destinationConversationID: "

WANTED_CONVERSATION_FIELDS = ("Title", "Actor", "Conversant")


def decode_scalar(value):
    """Decode a YAML flow scalar as Unity writes it: plain or single-quoted."""
    value = value.strip()
    if len(value) >= 2 and value[0] == "'" and value[-1] == "'":
        return value[1:-1].replace("''", "'")
    if len(value) >= 2 and value[0] == '"' and value[-1] == '"':
        return value[1:-1].replace('\\"', '"').replace("\\\\", "\\")
    return value


def read_conversations(path):
    """Yields one dict per conversation, in file order."""
    conversation = None
    entry = None
    # Which field's value line is expected next, for the two-line "title: X" then
    # "value: Y" shape Unity writes fields in.
    pending_conversation_field = None
    pending_entry_field = None
    inside = False

    with open(path, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.rstrip("\n")

            if not inside:
                inside = line == CONVERSATIONS
                continue

            if line == NEXT_SECTION:
                break

            if line.startswith(CONVERSATION_START):
                if conversation is not None:
                    if entry is not None:
                        conversation["entries"].append(entry)
                    yield conversation
                entry = None
                conversation = {
                    "id": int(line[len(CONVERSATION_START) :]),
                    "title": None,
                    "actor": None,
                    "conversant": None,
                    "entries": [],
                }
                continue

            if conversation is None:
                continue

            if line.startswith(ENTRY_START):
                if entry is not None:
                    conversation["entries"].append(entry)
                entry = {
                    "id": int(line[len(ENTRY_START) :]),
                    "group": False,
                    "actor": None,
                    "guard": "",
                    "script": "",
                    "to": [],
                    "title": None,
                    # Keep every field, not a hand-maintained subset. The offline
                    # crawler needs several presence-based fields (DifficultyPass,
                    # DifficultyRed, kim_watch, and others), and retaining them all
                    # makes a newer graph model possible without regenerating an
                    # index from the 170 MB source asset.
                    "fields": {},
                }
                continue

            if entry is None:
                if line.startswith(CONVERSATION_FIELD):
                    name = decode_scalar(line[len(CONVERSATION_FIELD) :])
                    pending_conversation_field = name if name in WANTED_CONVERSATION_FIELDS else None
                elif line.startswith(CONVERSATION_VALUE) and pending_conversation_field:
                    value = decode_scalar(line[len(CONVERSATION_VALUE) :])
                    key = pending_conversation_field.lower()
                    conversation[key] = value if key == "title" else as_int(value)
                    pending_conversation_field = None
                continue

            if line.startswith(ENTRY_FIELD):
                name = decode_scalar(line[len(ENTRY_FIELD) :])
                pending_entry_field = name
            elif line.startswith(ENTRY_VALUE) and pending_entry_field:
                value = decode_scalar(line[len(ENTRY_VALUE) :])
                entry["fields"][pending_entry_field] = value
                if pending_entry_field == "Title":
                    entry["title"] = value
                pending_entry_field = None
            elif line.startswith(ENTRY_IS_GROUP):
                entry["group"] = line[len(ENTRY_IS_GROUP) :].strip() == "1"
            elif line.startswith(ENTRY_CONDITIONS):
                entry["guard"] = decode_scalar(line[len(ENTRY_CONDITIONS) :])
            elif line.startswith(ENTRY_SCRIPT):
                entry["script"] = decode_scalar(line[len(ENTRY_SCRIPT) :])
            elif line.startswith(LINK_DESTINATION):
                entry["to"].append(int(line[len(LINK_DESTINATION) :]))
            elif line.startswith(LINK_DESTINATION_CONVERSATION):
                entry.setdefault("to_conversation", []).append(int(line[len(LINK_DESTINATION_CONVERSATION) :]))

    if conversation is not None:
        if entry is not None:
            conversation["entries"].append(entry)
        yield conversation


def as_int(value):
    """The database writes numbers as strings; a missing one stays None."""
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def extract_index(asset, out_path):
    written = 0
    with open(out_path, "w", encoding="utf-8") as handle:
        for conversation in read_conversations(asset):
            handle.write(json.dumps(conversation, separators=(",", ":")) + "\n")
            written += 1
    print(f"wrote {written} conversations to {out_path}")


###############################################################################
# CLI
###############################################################################

DEFAULT_ASSET = os.path.join(
    ".game_reference_copies",
    "AssetRipperExport",
    "ExportedProject",
    "Assets",
    "Dialogue Databases",
    "Disco Elysium.asset",
)
DEFAULT_OUT = os.path.join(".game_reference_copies", "derived", "conversation_index.jsonl")


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--asset", default=DEFAULT_ASSET, help="The database .asset")
    parser.add_argument("--out", default=DEFAULT_OUT, help="Where to write the index")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        os.makedirs(os.path.dirname(args.out), exist_ok=True)
        extract_index(args.asset, args.out)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
