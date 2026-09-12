#!/usr/bin/env python

"""Which weather preset index is rain and which is snow.

WeatherController.weatherPresets is an ordered list of asset references in Init.unity, and a
save records the INDEX of the one it was in. Each referenced asset carries a `type`, which
WeatherType spells CLEAR 0, RAIN 1, SNOW 2 - and which the controller copies into
auto.is_raining and auto.is_snowing whenever the weather changes.
"""

import argparse
import os
import re
import sys
import traceback

TYPES = {0: "CLEAR", 1: "RAIN", 2: "SNOW"}


def preset_guids(scene, script_guid):
    """The guids of weatherPresets, in the order the array lists them."""
    text = open(scene, encoding="utf-8", errors="replace").read()
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
            head = open(path, encoding="utf-8", errors="replace").read(400)
            guid = re.search(r"guid: ([0-9a-f]{32})", head)
            if guid:
                found[guid.group(1)] = path[: -len(".meta")]
    return found


def survey(scene, root, script_guid):
    guids = preset_guids(scene, script_guid)
    assets = assets_by_guid(root)

    print(f"{len(guids)} weather presets\n")
    for index, guid in enumerate(guids):
        path = assets.get(guid)
        kind, name = "?", "(asset not found)"
        if path:
            name = os.path.basename(path)[: -len(".asset")]
            body = open(path, encoding="utf-8", errors="replace").read()
            found = re.search(r"^  type: (\d+)", body, re.M)
            if found:
                kind = TYPES.get(int(found.group(1)), found.group(1))
        print(f"  {index:3}  {kind:6}  {name}")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scene")
    parser.add_argument("root")
    parser.add_argument("--script-guid", default="4d66ca46c163131855f2a6bc5b7dcf7a")
    args = parser.parse_args(argv if argv is not None else sys.argv[1:])
    try:
        survey(args.scene, args.root, args.script_guid)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
