#!/usr/bin/env python

"""Which weather each preset index is, read out of the assets the game reads.

## What this answers, and why it cannot be guessed

A save records the weather as an INDEX into `WeatherController.weatherPresets`, an ordered
list of asset references in `Init.unity`. The index on its own says nothing: the presets are
named for how they look rather than for what they are, and `RainClear_0` is CLEAR while
`SnowClear_0` is SNOW. Each referenced asset carries a `type`, which `WeatherType` spells
CLEAR 0, RAIN 1, SNOW 2.

## Why it matters beyond the name

THE PRESET IS THE TRUTH AND THE LUA VARIABLES ARE COPIES OF IT. On every weather change the
controller writes

    LuaHelper.SetVariable("auto.is_raining", weatherPresets[weatherPresetB].type == RAIN)

and the persister triggers the weather on load - so a save whose preset and whose variables
disagree is a save that will contradict itself the moment it is read. The dialogue guards
call `IsRaining()`, which reads the variable, so an offline world reads the variable too;
this table is what lets a test say the two halves of a save agree.

## Where the assets come from

The AssetRipper export of the installed game, which is not committed. The table it produces
IS, because it is small and because the fixtures need it.
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

DEFAULT_SCENE = os.path.join(
    ".game_reference_copies",
    "AssetRipperExport",
    "ExportedProject",
    "Assets",
    "Scenes",
    "Init.unity",
)

DEFAULT_ROOT = os.path.join(".game_reference_copies", "AssetRipperExport", "ExportedProject", "Assets")

DEFAULT_OUT = os.path.join("testing", "weather.json")

FORMAT = "weather-presets"
FORMAT_VERSION = 1

# The guid of WeatherController.cs, which is what finds the controller in the scene.
SCRIPT_GUID = "4d66ca46c163131855f2a6bc5b7dcf7a"

TYPES = {0: "CLEAR", 1: "RAIN", 2: "SNOW"}


def preset_guids(scene, script_guid):
    """The guids of weatherPresets, in the order the array lists them."""
    with open(scene, encoding="utf-8", errors="replace") as handle:
        text = handle.read()

    at = text.find(script_guid)
    if at < 0:
        raise RuntimeError(f"{script_guid} is not in {scene}")

    found = re.search(r"^  weatherPresets:\n((?:  - .*\n)*)", text[at:], re.M)
    if not found:
        raise RuntimeError("the controller lists no weatherPresets")
    return re.findall(r"guid: ([0-9a-f]{32})", found.group(1))


def assets_by_guid(root):
    """Every .asset in the export, keyed by the guid of its .meta."""
    found = {}
    for here, _, names in os.walk(root):
        for name in names:
            if not name.endswith(".asset.meta"):
                continue
            path = os.path.join(here, name)
            with open(path, encoding="utf-8", errors="replace") as handle:
                head = handle.read(400)
            guid = re.search(r"guid: ([0-9a-f]{32})", head)
            if guid:
                found[guid.group(1)] = path[: -len(".meta")]
    return found


def presets(scene, root, script_guid):
    """Each preset index, as the name it carries and the weather it is."""
    guids = preset_guids(scene, script_guid)
    assets = assets_by_guid(root)

    found = []
    for index, guid in enumerate(guids):
        path = assets.get(guid)
        if not path:
            raise RuntimeError(f"preset {index} references {guid}, which is not in {root}")

        with open(path, encoding="utf-8", errors="replace") as handle:
            body = handle.read()
        kind = re.search(r"^  type: (\d+)", body, re.M)
        if not kind:
            raise RuntimeError(f"preset {index} ({path}) carries no type")

        number = int(kind.group(1))
        if number not in TYPES:
            raise RuntimeError(f"preset {index} ({path}) has type {number}, which is not a weather")

        found.append((os.path.basename(path)[: -len(".asset")], TYPES[number]))

    return found


def write_table(scene, root, script_guid, out):
    found = presets(scene, root, script_guid)

    # BY INDEX RATHER THAN GROUPED BY TYPE, because the index is what a save records and a
    # second arrangement of the same facts is one more thing that can disagree with itself.
    document = {
        "_format": FORMAT,
        "_formatVersion": FORMAT_VERSION,
        "names": [name for name, _ in found],
        "types": [kind for _, kind in found],
    }

    os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(document, handle, indent=2, ensure_ascii=False)
        handle.write("\n")

    for index, (name, kind) in enumerate(found):
        print(f"  {index:3}  {kind:6}  {name}")
    print(f"{len(found)} weather presets written to {out}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--scene", default=DEFAULT_SCENE, help="the scene holding the controller")
    parser.add_argument("--root", default=DEFAULT_ROOT, help="the exported Assets directory")
    parser.add_argument("--script-guid", default=SCRIPT_GUID, help="WeatherController.cs's guid")
    parser.add_argument("--out", default=DEFAULT_OUT, help="where the table goes")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        write_table(args.scene, args.root, args.script_guid, args.out)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
