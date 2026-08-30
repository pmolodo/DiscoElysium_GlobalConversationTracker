#!/usr/bin/env -S uv run --script

# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

"""Extract the distinct guards and actions from a Dialogue System database .asset.

The output feeds the opt-in corpus tests in
src/GlobalConversationTracker.LookAhead.Tests/CorpusTests.cs, which parse every
condition and action the shipped game actually contains rather than a handful
someone thought to write down.

Two files are produced under .game_reference_copies/derived/ (git-ignored,
because this is extracted game content):

  distinct_guards.txt    one conditionsString per line
  distinct_scripts.txt   one userScript per line

Both are written one record per line with newlines and carriage returns escaped
as \\n and \\r, so a reader can split on lines and unescape.

The .asset is a Unity-serialized YAML document, streamed rather than parsed as
YAML: it is ~170 MB, and the fields wanted here sit at fixed indentation.
"""

import argparse
import os
import sys
import traceback

###############################################################################
# Core functions
###############################################################################

ENTRY_START = "    - id: "
CONDITIONS = "      conditionsString: "
USER_SCRIPT = "      userScript: "

DEFAULT_OUT = os.path.join(".game_reference_copies", "derived")


def decode_scalar(value):
    """Decode a YAML flow scalar as Unity writes it: plain, single- or double-quoted."""
    if len(value) >= 2 and value[0] == '"' and value[-1] == '"':
        body = value[1:-1]
        out = []
        index = 0
        while index < len(body):
            char = body[index]
            if char == "\\" and index + 1 < len(body):
                nxt = body[index + 1]
                out.append({"n": "\n", "t": "\t", "r": "\r"}.get(nxt, nxt))
                index += 2
            else:
                out.append(char)
                index += 1
        return "".join(out)
    if len(value) >= 2 and value[0] == "'" and value[-1] == "'":
        return value[1:-1].replace("''", "'")
    return value


def escape(text):
    """One record per line: the only characters that may not survive are the breaks."""
    return text.replace("\\", "\\\\").replace("\r", "\\r").replace("\n", "\\n")


def extract(asset_path):
    """Return (guards, scripts) as sorted lists of distinct non-empty strings."""
    guards = set()
    scripts = set()
    inside_entry = False

    with open(asset_path, "r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.rstrip("\n")
            if line.startswith(ENTRY_START):
                inside_entry = True
                continue
            if not inside_entry:
                continue
            if line.startswith(CONDITIONS):
                value = decode_scalar(line[len(CONDITIONS) :])
                if value.strip():
                    guards.add(value)
            elif line.startswith(USER_SCRIPT):
                value = decode_scalar(line[len(USER_SCRIPT) :])
                if value.strip():
                    scripts.add(value)

    return sorted(guards), sorted(scripts)


def write(rows, path):
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        for row in rows:
            handle.write(escape(row) + "\n")


def extract_corpus(asset, out_dir=DEFAULT_OUT):
    guards, scripts = extract(asset)
    os.makedirs(out_dir, exist_ok=True)

    guard_path = os.path.join(out_dir, "distinct_guards.txt")
    script_path = os.path.join(out_dir, "distinct_scripts.txt")
    write(guards, guard_path)
    write(scripts, script_path)

    print(f"{len(guards):>6} distinct guards  -> {guard_path}")
    print(f"{len(scripts):>6} distinct scripts -> {script_path}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "asset",
        help=(
            "Path to the Dialogue Database .asset (e.g. "
            "'.game_reference_copies/AssetRipperExport/ExportedProject/Assets/"
            "Dialogue Databases/Disco Elysium.asset')"
        ),
    )
    parser.add_argument(
        "--out-dir",
        default=DEFAULT_OUT,
        help="Directory to write the corpus files into",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        extract_corpus(args.asset, out_dir=args.out_dir)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
