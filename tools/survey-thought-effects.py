#!/usr/bin/env python

"""Which thought carries which CharacterEffect, read out of the prefab the game ships.

A thought is a ThoughtCabinetProject component on a GameObject inside Sunshine Data.prefab.
It names two arrays of CharacterEffect components by fileID: researchEffects, which apply
while the thought is cooking, and completionEffects, which apply once it is internalised.
Each effect carries a type, an ability, a skill and a parameter.
"""

import argparse
import re
import sys
import traceback

from collections import defaultdict

THOUGHT_GUID = "c2ddfc1347a0ef15cb45142256df7a6c"
EFFECT_GUID = "8722aa5be60ff77924e39179ac22f62c"

EFFECT_NAMES = {
    0: "NONE",
    1: "STAT_BONUS",
    2: "SKILL_BONUS",
    3: "SET_VARIABLE",
    4: "DAMAGE",
    5: "HEAL",
    6: "THC_ORB_MONEY",
    7: "THC_ORB_XP",
    8: "THC_CRIT_RANGE_EXPAND",
    9: "THC_RED_CHECK_FAILURE",
    10: "MALE_TARGET",
    11: "FEMALE_TARGET",
    12: "KIM_TARGET",
    13: "REOPEN_WHITE",
    14: "MOUTH_SLOT",
    15: "TOOLTIP",
    16: "MAX_LEARNING_CAP",
    17: "COMMUNISM_XP_BONUS",
    18: "SKILL_BONUS_WITHOUT_SHIRT",
    19: "SKILL_BONUS_WHEN_UNARMED",
    20: "PASSIVES_SUCCEED",
    21: "DRUGS_ARE_BAD_MKAY",
    22: "PASSIVE_TARGET_MODIFIER",
    23: "XP_REWARD",
    24: "LUA_COMMAND",
    25: "FIND_BETTER_ITEMS",
    26: "REPUTATION_BONUS",
    27: "MODIFY_CAMERA_MAX_ZOOM_LIMIT",
}
ABILITIES = {0: "-", 1: "INT", 2: "PSY", 3: "FYS", 4: "MOT"}

DOC = re.compile(r"^--- !u!\d+ &(\d+)", re.M)


def documents(text):
    """Each YAML document of the prefab, as (fileID, body)."""
    marks = list(DOC.finditer(text))
    for at, mark in enumerate(marks):
        end = marks[at + 1].start() if at + 1 < len(marks) else len(text)
        yield mark.group(1), text[mark.end() : end]


def scalar(body, name, default=None):
    found = re.search(rf"^  {re.escape(name)}: (.*)$", body, re.M)
    return found.group(1).strip() if found else default


def listed(body, name):
    """The fileIDs of a sequence field."""
    found = re.search(rf"^  {re.escape(name)}:\n((?:  - .*\n)*)", body, re.M)
    if not found:
        return []
    return re.findall(r"fileID: (\d+)", found.group(1))


def read(path):
    text = open(path, encoding="utf-8", errors="replace").read()

    thoughts, effects = {}, {}
    for file_id, body in documents(text):
        if THOUGHT_GUID in body:
            thoughts[file_id] = body
        elif EFFECT_GUID in body:
            effects[file_id] = body
    return thoughts, effects


def named(body):
    """The thought's id, taken from the localisation term that carries it."""
    found = re.search(r"Term: Thoughts/([^/\s]+)/", body)
    return found.group(1) if found else "?"


def effect_of(body):
    return {
        "effect": EFFECT_NAMES.get(int(scalar(body, "effect", "0")), "?"),
        "ability": ABILITIES.get(int(scalar(body, "abilityType", "0")), "?"),
        "skill": int(scalar(body, "skillType", "0")),
        "parameter": int(scalar(body, "parameter", "0")),
    }


def survey(path, only):
    thoughts, effects = read(path)
    print(f"{len(thoughts)} thoughts, {len(effects)} effects in {path}\n")

    by_effect = defaultdict(list)
    for body in thoughts.values():
        name = named(body)
        for when in ("researchEffects", "completionEffects"):
            for file_id in listed(body, when):
                if file_id not in effects:
                    continue
                one = effect_of(effects[file_id])
                by_effect[one["effect"]].append((name, when, one))

    wanted = only or sorted(by_effect)
    for kind in wanted:
        rows = by_effect.get(kind, [])
        print(f"{kind}: {len(rows)}")
        for name, when, one in rows:
            print(
                f"    {name:28} {when:18} ability={one['ability']:4} "
                f"skill={one['skill']:3} parameter={one['parameter']}"
            )


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("prefab")
    parser.add_argument("--only", action="append")
    args = parser.parse_args(argv if argv is not None else sys.argv[1:])
    try:
        survey(args.prefab, args.only)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
