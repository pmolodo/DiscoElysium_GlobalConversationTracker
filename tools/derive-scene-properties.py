#!/usr/bin/env python

"""Which of the game's scenes are outdoors, read out of the asset the game reads.

## What this answers, and why it cannot be guessed

`IsExterior()` returns `ApplicationManager.CurrentSceneProperties.IsOutside`, and
`CurrentSceneProperties` is the entry of `ScenePropertiesList` whose `SceneId` matches the
area the player is in. A save records that area id, so an offline world can answer the
query - given this table.

THE SUFFIX ON A SCENE ID IS NOT THE ANSWER, even though it looks like one. Measured over
the shipped list: 30 of 37 scene ids end in `-int` or `-ext` and every one of those agrees
with its flag, but 7 say nothing either way. Two scenes in the whole game are outdoors.
The table is written down rather than derived from the name so that a scene added or
renamed cannot quietly change what an offline run believes.

## Where the asset comes from

The AssetRipper export of the installed game, which is not committed. The table it produces
IS, because it is small and because every offline run needs it.
"""

import argparse
import json
import os
import re
import sys
import traceback

###############################################################################
# Core functions
###############################################################################

DEFAULT_ASSET = os.path.join(
    ".game_reference_copies",
    "AssetRipperExport",
    "ExportedProject",
    "Assets",
    "MonoBehaviour",
    "Application Manager.asset",
)

DEFAULT_OUT = os.path.join("testing", "scenes.json")

FORMAT = "scenes"
FORMAT_VERSION = 1

# One entry of ScenePropertiesList, whose fields the exporter writes in declaration order.
ENTRY = re.compile(
    r"- SceneName: (?P<name>.*)\n"
    r"\s+SceneId: (?P<id>.*)\n"
    r"\s+SaveGameId: .*\n"
    r"\s+VisualRadius: .*\n"
    r"\s+IsOutside: (?P<outside>[01])"
)


def scene_properties(asset):
    """Every scene the game knows, as its id and whether it is outdoors."""
    with open(asset, encoding="utf-8") as handle:
        text = handle.read()

    found = {}
    for match in ENTRY.finditer(text):
        scene = match.group("id").strip()
        if scene in found:
            raise RuntimeError(f"{asset} lists '{scene}' twice")
        found[scene] = match.group("outside") == "1"

    if not found:
        raise RuntimeError(f"{asset} holds no ScenePropertiesList entries")
    return found


def write_table(asset, out):
    found = scene_properties(asset)
    outside = sorted(scene for scene, is_outside in found.items() if is_outside)

    document = {
        "_format": FORMAT,
        "_formatVersion": FORMAT_VERSION,
        "outside": outside,
        "scenes": sorted(found),
    }

    os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(document, handle, indent=2, ensure_ascii=False)
        handle.write("\n")

    print(f"{len(found)} scenes, {len(outside)} of them outdoors, written to {out}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--asset", default=DEFAULT_ASSET, help="the Application Manager asset")
    parser.add_argument("--out", default=DEFAULT_OUT, help="where the table goes")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        write_table(args.asset, args.out)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
