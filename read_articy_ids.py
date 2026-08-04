#!/usr/bin/env -S uv run --script

# /// script
# requires-python = ">=3.10"
# dependencies = ["yamlrocks", "tqdm"]
# ///

"""Build articy-id lookup tables from a Dialogue System database .asset file.

The input is a Unity-serialized Dialogue Database (e.g.
"Dialogue Databases/Disco Elysium.asset"), read as a YAML document. Two mappings
are produced:

  - articy_to_conversation:   articy_id -> conversation_id
  - articy_to_dialogue_entry: articy_id -> [conversation_id, [dialogue_entry_id, ...]]

Multiple dialogue entries can share one articy id (in this database ~8.4k do),
but only ever within a single conversation, so the entry mapping groups all
matching entry ids under their common conversation id.

For every conversation and every dialogue entry, the articy id is the value of
the field whose title is "Articy Id". This script errors (fails fast) if:

  - any conversation or dialogue entry lacks an id,
  - any conversation or dialogue entry lacks an Articy Id,
  - a dialogue entry's conversationID does not match its parent conversation id,
  - two conversations collide on the same articy id,
  - one articy id is shared by dialogue entries in different conversations.
"""

import argparse
import json
import os
import sys
import traceback

import yamlrocks

from tqdm import tqdm

FIELD_TITLE_KEY = "title"
FIELD_VALUE_KEY = "value"
ARTICY_ID_TITLE = "Articy Id"

# Articy ids are 64-bit values, rendered in the source as 0x + 16 hex digits,
# sometimes with a suffix (e.g. "0x0000000000000002-START", "...-FORK").
ARTICY_ID_HEX_WIDTH = 16


###############################################################################
# YAML loading
###############################################################################


def _keep_tagged_value(tag, value):
    """Catch-all tag handler: use the constructed value as-is.

    The database is a Unity asset whose document carries an application-specific
    tag (e.g. !u!114); yamlrocks routes any unregistered tag through here, and we
    simply keep the underlying mapping/sequence/scalar.
    """
    return value


def load_asset(input_path):
    # yamlrocks parses and builds the object graph in native code (~25x faster
    # than PyYAML's libyaml loader on this ~140 MB file), and honors the file's
    # "%YAML 1.1" directive. It does the parse in a single native call, so there
    # is no Python-level hook for a determinate bar here; the progress bar is on
    # the mapping build below instead.
    size_mb = os.path.getsize(input_path) / 1e6
    tqdm.write(f"Parsing {os.path.basename(input_path)} ({size_mb:.0f} MB) with yamlrocks...")
    return yamlrocks.load(input_path, tag_handler=_keep_tagged_value)


###############################################################################
# Core functions
###############################################################################


def normalize_articy_id(value):
    """Return the articy id as a string.

    Pure-hex ids are parsed by the YAML loader as ints; render those as a
    canonical 0x-prefixed, upper-case, 16-digit hex string. Ids that carry a
    suffix (e.g. "0x0000000000000002-START") arrive as strings and are used
    as-is.
    """
    if isinstance(value, int):
        return f"0x{value:0{ARTICY_ID_HEX_WIDTH}X}"
    return str(value)


def get_articy_id(fields):
    """Return the normalized Articy Id from a fields list, or None if absent/empty."""
    for field in fields or []:
        if field.get(FIELD_TITLE_KEY) == ARTICY_ID_TITLE:
            value = field.get(FIELD_VALUE_KEY)
            if value is None or value == "":
                return None
            return normalize_articy_id(value)
    return None


def _add_unique(mapping, key, value, what):
    if key in mapping:
        raise ValueError(f"Duplicate Articy Id {key} for {what} (already mapped to {mapping[key]!r})")
    mapping[key] = value


def build_mappings(data):
    """Return (articy_to_conversation, articy_to_dialogue_entry) from parsed asset data."""
    conversations = data["MonoBehaviour"]["conversations"]

    articy_to_conversation = {}
    articy_to_dialogue_entry = {}

    for conversation in tqdm(conversations, desc="Building mappings", unit=" conv"):
        conversation_id = conversation.get("id")
        if conversation_id is None:
            raise ValueError(f"Conversation is missing an 'id': {conversation!r}")

        articy_id = get_articy_id(conversation.get("fields"))
        if articy_id is None:
            raise ValueError(f"Conversation {conversation_id} is missing an Articy Id")
        _add_unique(articy_to_conversation, articy_id, conversation_id, f"conversation {conversation_id}")

        for entry in conversation.get("dialogueEntries") or []:
            entry_id = entry.get("id")
            if entry_id is None:
                raise ValueError(f"Dialogue entry in conversation {conversation_id} is missing an 'id': {entry!r}")

            entry_conversation_id = entry.get("conversationID")
            if entry_conversation_id != conversation_id:
                raise ValueError(
                    f"Dialogue entry {entry_id} has conversationID {entry_conversation_id!r}, "
                    f"which does not match parent conversation id {conversation_id}"
                )

            entry_articy_id = get_articy_id(entry.get("fields"))
            if entry_articy_id is None:
                raise ValueError(f"Dialogue entry {conversation_id}:{entry_id} is missing an Articy Id")
            existing = articy_to_dialogue_entry.get(entry_articy_id)
            if existing is None:
                articy_to_dialogue_entry[entry_articy_id] = [conversation_id, [entry_id]]
            else:
                existing_conversation_id, entry_ids = existing
                if existing_conversation_id != conversation_id:
                    raise ValueError(
                        f"Articy Id {entry_articy_id} is shared by dialogue entries in different "
                        f"conversations ({existing_conversation_id} and {conversation_id})"
                    )
                entry_ids.append(entry_id)

    return {"conversations": articy_to_conversation, "dialogue_entries": articy_to_dialogue_entry}


###############################################################################
# CLI
###############################################################################


def read_and_output_articy_ids(input_path, output_path=None, indent=2):
    data = load_asset(input_path)
    result = build_mappings(data)
    text = json.dumps(result, indent=indent, ensure_ascii=False)
    if output_path:
        with open(output_path, "w", encoding="utf-8") as f:
            f.write(text)
    else:
        print(text)


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("input", help="Path to the Dialogue Database .asset file")
    parser.add_argument("-o", "--output", help="Output JSON path (default: stdout)")
    parser.add_argument("--indent", type=int, default=2, help="JSON indentation width")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        read_and_output_articy_ids(args.input, args.output, args.indent)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
