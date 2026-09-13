#!/usr/bin/env python

"""Which thoughts change passive checks, and how, read out of the prefab the game ships.

## What this answers, and why the save cannot

A passive check's threshold, and whether it passes at all, can depend on the thoughts the
player holds. The game applies that through `ThoughtAlterant.ModifyPassiveTargetValue` and
`ThoughtAlterant.PassiveSuccess`, reading the live cabinet. A save records only which thought
is in which state; what a thought DOES is a CharacterEffect on its definition in
`Sunshine Data.prefab`. So an offline fixture needs this table beside the save.

Two effect types touch passive checks, and three thoughts in the whole game carry them:

- PASSIVE_TARGET_MODIFIER moves the threshold of every passive check whose skill belongs to
  one ability, by the effect's parameter.
- PASSIVES_SUCCEED forces every passive check of one skill through.

## What was measured, because the prefab does not say it

The prefab gives an ability, a skill INDEX and a parameter; it does not say which way the
parameter moves a threshold, which thought state applies it, or which skill an index is.
All three were measured in game on 2026-09-13 (de-2jlj): at-trashcan against the same save
with lawbringer, remote_viewer and age_bracket FIXED moved exactly nine checks from fail to
pass. A parameter of -1 lowers the threshold by one; the effects are completion effects and
apply to a FIXED thought; and PASSIVES_SUCCEED's skill index 20 is HAND/EYE COORDINATION.

A skill index this has not measured is refused rather than guessed, because a wrong guess
decides checks the wrong way and nothing downstream would say so.

## Where the prefab comes from

The AssetRipper export of the installed game, which is not committed. The table is.
"""

import argparse
import importlib.util
import json
import os
import sys
import traceback

###############################################################################
# Core functions
###############################################################################

HERE = os.path.dirname(os.path.abspath(__file__))

DEFAULT_PREFAB = os.path.join(
    ".game_reference_copies",
    "AssetRipperExport",
    "ExportedProject",
    "Assets",
    "Resources",
    "prefabs",
    "Sunshine Data.prefab",
)

DEFAULT_OUT = os.path.join("testing", "thought-effects.json")

FORMAT = "thought-effects"
FORMAT_VERSION = 1

TARGET_MODIFIER = "PASSIVE_TARGET_MODIFIER"
SUCCEEDS = "PASSIVES_SUCCEED"

# Which field of a thought lists the effects, and when the game applies them.
PHASES = {"researchEffects": "research", "completionEffects": "completion"}

# The SkillType a CharacterEffect's skill index stands for, for the indices a passive effect
# uses. Measured, not read: see the module doc.
SKILL_BY_INDEX = {20: "HE_COORDINATION"}


def survey_module():
    """The prefab reader `survey-thought-effects.py` already has, rather than a second one."""
    path = os.path.join(HERE, "survey-thought-effects.py")
    spec = importlib.util.spec_from_file_location("survey_thought_effects", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"{path} could not be loaded as a module")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def passive_effects(prefab):
    """Every effect on a thought that changes a passive check, as table rows."""
    survey = survey_module()
    thoughts, effects = survey.read(prefab)

    rows = []
    for body in thoughts.values():
        thought = survey.named(body)
        for field, phase in PHASES.items():
            for file_id in survey.listed(body, field):
                if file_id not in effects:
                    continue
                effect = survey.effect_of(effects[file_id])
                kind = effect["effect"]
                if kind == TARGET_MODIFIER:
                    if effect["ability"] in ("-", "?"):
                        raise RuntimeError(f"{thought}: a {kind} that names no ability")
                    rows.append(
                        {
                            "ability": effect["ability"],
                            "amount": effect["parameter"],
                            "effect": kind,
                            "phase": phase,
                            "thought": thought,
                        }
                    )
                elif kind == SUCCEEDS:
                    index = effect["skill"]
                    if index not in SKILL_BY_INDEX:
                        raise RuntimeError(
                            f"{thought}: a {kind} for skill index {index}, which has not been "
                            "measured in game - see the module doc before adding it"
                        )
                    rows.append(
                        {
                            "effect": kind,
                            "phase": phase,
                            "skill": SKILL_BY_INDEX[index],
                            "thought": thought,
                        }
                    )

    if not rows:
        raise RuntimeError(f"{prefab} carries no passive-check effect on any thought")
    rows.sort(key=lambda row: (row["thought"], row["phase"], row["effect"]))
    return rows


def write_table(prefab, out):
    rows = passive_effects(prefab)
    document = {
        "_format": FORMAT,
        "_formatVersion": FORMAT_VERSION,
        "effects": rows,
    }

    os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(document, handle, indent=2, ensure_ascii=False, sort_keys=True)
        handle.write("\n")

    print(f"{len(rows)} passive-check effects, written to {out}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--prefab", default=DEFAULT_PREFAB, help="the Sunshine Data prefab")
    parser.add_argument("--out", default=DEFAULT_OUT, help="where the table goes")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        write_table(args.prefab, args.out)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
