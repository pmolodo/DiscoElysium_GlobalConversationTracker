#!/usr/bin/env python

"""Write the raining and snowing saves as one-variable changes to an outdoor one.

MADE RATHER THAN PLAYED, and that is the point: waiting in game for the weather to turn
means letting the clock run, and the clock moves the day, the thoughts cooking and whatever
else is on a timer - so two saves meant to differ in the sky would differ in a dozen things
and no run could say which one moved a marker. A hand-made diff differs in ONE VARIABLE.

What it writes is the smallest expanded save there is: a manifest inheriting every
pass-through member, and one sparse diff over the base's Variable table. The writer then
regenerates both properly - see the rewrite verb - so what lands in the repository is this
build's own output rather than this script's.
"""

import argparse
import json
import os
import pathlib
import sys
import traceback

###############################################################################
# Core functions
###############################################################################

BASE = "at-trashcan"
PARTS_SUFFIX = ".lua.parts"
EXPANDED_SUFFIX = ".ntwtf"

# What each variant changes, and nothing else.
WEATHER = {
    "scene-raining": "auto.is_raining",
    "scene-snowing": "auto.is_snowing",
}

# The members every save carries, which these inherit whole.
SUFFIXES = [".1st.ntwtf.json", ".2nd.ntwtf.json", ".FOW.json", ".states.lua"]


def manifest_for(name):
    return {
        "_format": "expanded-save-diff",
        "_formatVersion": 1,
        "base": f"../{BASE}{EXPANDED_SUFFIX}",
        "members": [
            {
                "diff": None,
                "kind": "inherit",
                "name": f"{name}{suffix}",
                "suffix": suffix,
            }
            for suffix in SUFFIXES
        ],
    }


def variable_diff(variable):
    """The one change, as a diff of the base's own Variable table."""
    return {
        "_format": "sparse-diff",
        "_formatVersion": 1,
        "_base": f"../../{BASE}{EXPANDED_SUFFIX}/{BASE}{EXPANDED_SUFFIX}{PARTS_SUFFIX}/Variable.json",
        "_changes": {variable: True},
    }


def write_json(path, document):
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(document, handle, indent=2, ensure_ascii=False)
        handle.write("\n")


def make(repo):
    scenarios = pathlib.Path(repo) / "testing" / "scenarios"
    base = scenarios / f"{BASE}{EXPANDED_SUFFIX}"
    if not base.is_dir():
        raise RuntimeError(f"{base} is not there, and it is what these are a change to")

    for name, variable in WEATHER.items():
        save = scenarios / f"{name}{EXPANDED_SUFFIX}"
        if save.exists():
            raise RuntimeError(f"{save} is already there; remove it first")

        write_json(save / "_archive.json", manifest_for(name))
        write_json(
            save / f"{name}{EXPANDED_SUFFIX}{PARTS_SUFFIX}" / "Variable.json",
            variable_diff(variable),
        )
        print(f"  {name}: {variable} = true, everything else inherited")

    print(f"{len(WEATHER)} saves written as a change to {BASE}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--repo", default=os.getcwd(), help="the repository root")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        make(args.repo)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
