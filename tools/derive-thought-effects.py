#!/usr/bin/env python

"""Which thoughts change what an offline world answers, and how, read out of the prefab the game ships.

## What this answers, and why the save cannot

A save records which thought is in which state, and the results of some effects - but not
what a thought DOES while it is held. That is a CharacterEffect on the thought's definition
in `Sunshine Data.prefab`, and the game re-applies it from there every time a save loads. So
an offline fixture needs this table beside the save.

## Every effect type, and what an offline world makes of it

Read from the decompiled game - `CharacterEffect.Apply` for what each type does,
`CharacterSheetPersister.ThoughtEffectShouldPersist` for which a load re-applies - and held
against what the look-ahead is asked: the plugin consults only the two passive-check hooks,
and every white and red check is carried both ways whatever its odds.

- APPLIED, and so written to the table: PASSIVE_TARGET_MODIFIER moves the threshold of every
  passive check whose skill belongs to one ability; PASSIVES_SUCCEED forces every passive
  check of one skill through.
- Everything else is classified in NOT_APPLIED with its reason. A type in neither is refused,
  so a game patch that adds one stops this tool rather than being silently ignored.

## What was measured, because the prefab does not say it

The prefab gives an ability, a skill index and a parameter; it does not say which way the
parameter moves a threshold or which thought state applies it. Both were measured in game on
2026-09-13 (de-2jlj): at-trashcan against the same save with lawbringer, remote_viewer and
age_bracket FIXED moved exactly nine checks from fail to pass. A parameter of -1 lowers the
threshold by one, and the effects are completion effects applied to a FIXED thought. Which
skill an index is comes from the game's `SkillType` enum, which agrees with the measured
index 20, Hand/Eye Coordination.

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
REPUTATION_BONUS = "REPUTATION_BONUS"

# Which field of a thought lists the effects, and when the game applies them.
PHASES = {"researchEffects": "research", "completionEffects": "completion"}

IN_THE_SAVE = "the save already holds its result, and loading one does not re-apply it"
ROLLED_ONLY = "moves only a white or red check's target or odds, which the look-ahead carries both ways"
NOT_ASKED = "changes nothing a dialogue guard or check reads"

# Every effect type that is deliberately not applied offline, and why.
NOT_APPLIED = {
    "COMMUNISM_XP_BONUS": NOT_ASKED,
    "DAMAGE": IN_THE_SAVE,
    "DRUGS_ARE_BAD_MKAY": NOT_ASKED,
    "FEMALE_TARGET": ROLLED_ONLY,
    "FIND_BETTER_ITEMS": NOT_ASKED,
    "HEAL": IN_THE_SAVE,
    "KIM_TARGET": ROLLED_ONLY,
    "LUA_COMMAND": IN_THE_SAVE,
    "MALE_TARGET": ROLLED_ONLY,
    "MAX_LEARNING_CAP": NOT_ASKED,
    "MODIFY_CAMERA_MAX_ZOOM_LIMIT": NOT_ASKED,
    "MOUTH_SLOT": NOT_ASKED,
    "REOPEN_WHITE": IN_THE_SAVE,
    "SKILL_BONUS": IN_THE_SAVE,
    "SKILL_BONUS_WHEN_UNARMED": ROLLED_ONLY,
    "SKILL_BONUS_WITHOUT_SHIRT": ROLLED_ONLY,
    "STAT_BONUS": IN_THE_SAVE,
    "THC_CRIT_RANGE_EXPAND": ROLLED_ONLY,
    "THC_ORB_MONEY": NOT_ASKED,
    "THC_ORB_XP": NOT_ASKED,
    "THC_RED_CHECK_FAILURE": "forces every red check to fail, which the look-ahead does not model (de-kmtt.6)",
    "TOOLTIP": NOT_ASKED,
    "XP_REWARD": IN_THE_SAVE,
}

# The SkillType each index names, from the game's `Sunshine.Metric.SkillType` enum - the
# skills the character sheet holds. The four Perception senses, Convalescence and ALT are
# left out: the fixture collapses a sense onto Perception, so an effect naming one would be
# decided against a skill it does not name.
SKILL_BY_INDEX = {
    1: "LOGIC",
    2: "ENCYCLOPEDIA",
    3: "RHETORIC",
    4: "DRAMA",
    5: "CONCEPTUALIZATION",
    6: "VISUAL_CALCULUS",
    7: "VOLITION",
    8: "INLAND_EMPIRE",
    9: "EMPATHY",
    10: "AUTHORITY",
    11: "SUGGESTION",
    12: "ESPRIT_DE_CORPS",
    13: "PHYSICAL_INSTRUMENT",
    14: "ELECTROCHEMISTRY",
    15: "ENDURANCE",
    17: "HALF_LIGHT",
    18: "PAIN_THRESHOLD",
    19: "SHIVERS",
    20: "HE_COORDINATION",
    21: "PERCEPTION",
    26: "REACTION",
    27: "SAVOIR_FAIRE",
    28: "INTERFACING",
    29: "COMPOSURE",
}


def survey_module():
    """The prefab reader `survey-thought-effects.py` already has, rather than a second one."""
    path = os.path.join(HERE, "survey-thought-effects.py")
    spec = importlib.util.spec_from_file_location("survey_thought_effects", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"{path} could not be loaded as a module")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def applied_row(thought, phase, kind, effect, body, survey):
    """The table row for one effect, or None when it is classified as not applied."""
    if kind == TARGET_MODIFIER:
        if effect["ability"] in ("-", "?"):
            raise RuntimeError(f"{thought}: a {kind} that names no ability")
        return {
            "ability": effect["ability"],
            "amount": effect["parameter"],
            "effect": kind,
            "phase": phase,
            "thought": thought,
        }
    if kind == SUCCEEDS:
        index = effect["skill"]
        if index not in SKILL_BY_INDEX:
            raise RuntimeError(f"{thought}: a {kind} for skill index {index}, which is not a skill the sheet holds")
        return {
            "effect": kind,
            "phase": phase,
            "skill": SKILL_BY_INDEX[index],
            "thought": thought,
        }
    if kind == REPUTATION_BONUS:
        # A Lua write to reputation.<name>, which guards read. With no name the game looks for
        # a variable called "reputation." and, finding none, does nothing.
        reputation = survey.scalar(body, "stringParameter", "")
        if reputation:
            raise RuntimeError(f"{thought}: a {kind} for reputation.{reputation}, which is not handled")
        return None
    if kind in NOT_APPLIED:
        return None
    raise RuntimeError(f"{thought}: an effect type {kind} with no classification - see the module doc")


def applied_effects(prefab):
    """Every effect on a thought that changes what an offline world answers, as table rows."""
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
                row = applied_row(thought, phase, effect["effect"], effect, effects[file_id], survey)
                if row is not None:
                    rows.append(row)

    if not rows:
        raise RuntimeError(f"{prefab} carries no applied effect on any thought")
    rows.sort(key=lambda row: (row["thought"], row["phase"], row["effect"]))
    return rows


def write_table(prefab, out):
    rows = applied_effects(prefab)
    document = {
        "_format": FORMAT,
        "_formatVersion": FORMAT_VERSION,
        "effects": rows,
    }

    os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(document, handle, indent=2, ensure_ascii=False, sort_keys=True)
        handle.write("\n")

    print(f"{len(rows)} applied thought effects, written to {out}")


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
