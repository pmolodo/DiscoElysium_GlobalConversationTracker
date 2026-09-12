#!/usr/bin/env python

"""Write the weather saves as the smallest change to a save that is already in the clear.

MADE RATHER THAN PLAYED, and that is the point: waiting in game for the weather to turn
means letting the clock run, and the clock moves the day, the thoughts cooking and whatever
else is on a timer - so two saves meant to differ in the sky would differ in a dozen things
and no run could say which one moved a marker. A hand-made diff differs in the weather and
nothing else.

FOUR OF THEM, in two pairs. The outdoor pair hangs off the trash can save and is where the
weather guards can actually be reached; the indoor pair hangs off the save one door away and
is there to show they cannot. Conversation 29 only asks about the sky below the branch
IsExterior opens, so an indoor save in the rain should reach exactly what an indoor save in
the clear does - which is a claim worth a fixture, and not one the outdoor pair can make.

## TWO FIELDS, NOT ONE, AND THE PRESET IS THE ONE THAT DECIDES

The dialogue guards call IsRaining(), which reads the Lua variable `auto.is_raining` - so
setting that variable is what an offline world needs. IT IS NOT WHAT THE GAME NEEDS.
`WeatherController`, on every weather change, writes both variables from the preset it is
changing to:

    LuaHelper.SetVariable("auto.is_raining", weatherPresets[weatherPresetB].type == RAIN)

and `WeatherPersister` triggers the weather on load. So a save that sets only the variable
is overwritten the moment it is read. Measured in game 2026-09-12: mid-load scene-raining
answers IsRaining() true, and by the time its menu goes up both the query and the variable
are false again.

The preset therefore goes in too, and the two are set consistently. Preset indices come from
`tools/derive-weather-presets.py`, which resolves the controller's ordered array to each
preset's own type; the plain ones are used rather than the heavy or dramatic variants, since
what is wanted is the weather TYPE and not whatever else a dramatic preset drags with it.

What it writes is the smallest expanded save there is: a manifest inheriting the members it
does not touch, a sparse diff over the base's Variable table, and a JSON diff over the
world-state document that holds the preset. The writer then regenerates them properly - see
the rewrite verb - so what lands in the repository is this build's own output rather than
this script's.
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

PARTS_SUFFIX = ".lua.parts"
EXPANDED_SUFFIX = ".ntwtf"

RAINING = "auto.is_raining"
SNOWING = "auto.is_snowing"

# The preset each weather is, as its index in WeatherController.weatherPresets.
RAIN_PRESET = 1
SNOW_PRESET = 2

OUTDOORS = "at-trashcan"
INDOORS = "scene-indoors"

# What each variant is a change to, and the weather it changes it to.
WEATHER = {
    "scene-raining": (OUTDOORS, RAINING, RAIN_PRESET),
    "scene-snowing": (OUTDOORS, SNOWING, SNOW_PRESET),
    "scene-indoors-raining": (INDOORS, RAINING, RAIN_PRESET),
    "scene-indoors-snowing": (INDOORS, SNOWING, SNOW_PRESET),
}

# The member holding the preset, which is the one these change rather than inherit.
WORLD_SUFFIX = ".2nd.ntwtf.json"

# The members every save carries, which these inherit whole.
SUFFIXES = [".1st.ntwtf.json", WORLD_SUFFIX, ".FOW.json", ".states.lua"]


def member_for(name, suffix):
    """One manifest entry: a diff of its own where it has one, inherited where not."""
    if suffix != WORLD_SUFFIX:
        return {
            "diff": None,
            "kind": "inherit",
            "name": f"{name}{suffix}",
            "suffix": suffix,
        }

    return {
        "diff": f"{name}{suffix}",
        "kind": "json",
        "name": f"{name}{suffix}",
        "suffix": suffix,
    }


def manifest_for(name, base):
    return {
        "_format": "expanded-save-diff",
        "_formatVersion": 1,
        "base": f"../{base}{EXPANDED_SUFFIX}",
        "members": [member_for(name, suffix) for suffix in SUFFIXES],
    }


def variable_diff(base, variable):
    """What the guards read, as a diff of the base's own Variable table."""
    return {
        "_format": "sparse-diff",
        "_formatVersion": 2,
        "_base": f"../../{base}{EXPANDED_SUFFIX}/{base}{EXPANDED_SUFFIX}{PARTS_SUFFIX}/Variable.json",
        "_changes": {variable: True},
    }


def preset_diff(base, preset):
    """What the game reads, as a diff of the base's own world-state document."""
    return {
        "_format": "json-diff",
        "_formatVersion": 1,
        "_base": f"../{base}{EXPANDED_SUFFIX}/{base}{WORLD_SUFFIX}",
        "_changes": {"weatherState": {"weatherPreset": preset}},
    }


def write_json(path, document):
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(document, handle, indent=2, ensure_ascii=False)
        handle.write("\n")


def make(repo, wanted):
    scenarios = pathlib.Path(repo) / "testing" / "scenarios"

    for name in wanted:
        base, variable, preset = WEATHER[name]
        beneath = scenarios / f"{base}{EXPANDED_SUFFIX}"
        if not beneath.is_dir():
            raise RuntimeError(f"{beneath} is not there, and it is what {name} changes")

        save = scenarios / f"{name}{EXPANDED_SUFFIX}"
        if save.exists():
            raise RuntimeError(f"{save} is already there; remove it first")

        write_json(save / "_archive.json", manifest_for(name, base))
        write_json(
            save / f"{name}{EXPANDED_SUFFIX}{PARTS_SUFFIX}" / "Variable.json",
            variable_diff(base, variable),
        )
        write_json(save / f"{name}{WORLD_SUFFIX}", preset_diff(base, preset))
        print(f"  {name}: {variable} = true and preset {preset} over {base}, everything else inherited")

    print(f"{len(wanted)} save(s) written")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--repo", default=os.getcwd(), help="the repository root")
    parser.add_argument(
        "--name",
        action="append",
        choices=sorted(WEATHER),
        help="write only this save; repeatable, and all of them by default",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        make(args.repo, args.name or sorted(WEATHER))
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
