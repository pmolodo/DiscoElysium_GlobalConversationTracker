#!/usr/bin/env -S uv run --script

# /// script
# requires-python = ">=3.10"
# dependencies = []
# ///

"""Build the in-game look-ahead scenarios: expanded saves plus one global state.

Every scenario is a small, stated diff against testing/save_template.ntwtf. Nothing
else about the template changes, so what the look-ahead sees differs only in the ways
the scenario names.

The scenarios exercise the forward scan's world simulation, which is the part no
unit test over a synthetic graph can prove is wired to the real game. Conversation
451 ("JAM / faln sneakers on a pedestal of speakers") gates a 0.50 real purchase
behind a 50.00 real one:

    22 (hub) --86 [cost 5000]--> 16 (sets jam.siileng_bought_faln_sneakers) --> ... --> 22
    22 (hub) --11 [cost   50, needs that variable]--> 80 --> 91 --> 22

The staged global state marks all 95 entries of conversation 451 as seen in another
playthrough EXCEPT entry 80, so 80 is the only unseen-anywhere node in the graph. An
option therefore earns an ORANGE marker exactly when the crawl can reach 80, and
reaching it means the crawl spent 50.00 and then still had 0.50:

    money 5100  ->  100 left, >= 50   ->  orange marker expected
    money 5025  ->   25 left,  < 50   ->  no orange marker
    money 4900  ->  cannot buy at all ->  no orange marker

The middle row is the whole point. A scan that checked affordability without
subtracting would call 80 reachable there, and be wrong.
"""

import argparse
import io
import json
import os
import shutil
import sys
import traceback

###############################################################################
# What the scenarios are
###############################################################################

TEMPLATE_NAME = "save_template"
CONVERSATION = 451
UNSEEN_ENTRY = 80

# Must match GlobalStateJson.FormatVersion.
GLOBAL_STATE_VERSION = 2

# Where the player stands. Lifted from Kim's own recorded Position_Martinaise_ext in
# a real save: about 1.3 units from Siileng's stall at -16.7,4.3,-51.3 and facing it.
# A position the game itself wrote for a character is known to be on the navmesh,
# which a coordinate read off a scene dump is not.
STAND_AT = "-15.41254,4.266205,-51.17798,0,0.6010301,0,-0.7992264,Martinaise-ext"
AREA_ID = "Martinaise-ext"
POSITION_FIELD = "Position_Martinaise_ext"

# Set so the sneakers option is offered and the speakers option is not gated on
# anything but the purchase the crawl has to simulate.
VARIABLES = {
    "jam.siileng_faln_sneakers": True,
    "jam.siileng_learned_when_you_can_buy_speakers": True,
}

# In centimes, as ClickCost is: the sneakers cost 5000 and the speakers 50.
SCENARIOS = {
    "afford-both": 5100,
    "afford-only-sneakers": 5025,
    "afford-neither": 4900,
}

###############################################################################
# Core functions
###############################################################################


def load(path):
    with io.open(path, encoding="utf-8") as handle:
        return json.load(handle)


def save(path, data):
    with io.open(path, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(data, handle, indent=4, ensure_ascii=False)
        handle.write("\n")


def player_actor(actors):
    """The one actor flagged IsPlayer, which is where the position lives."""
    players = [name for name, body in actors.items() if isinstance(body, dict) and body.get("IsPlayer") is True]
    if len(players) != 1:
        raise ValueError(f"Expected exactly one player actor, found {players}")
    return players[0]


def build_save(template_dir, out_dir, name, money):
    """Copies the template and applies one scenario's diff."""
    if os.path.exists(out_dir):
        shutil.rmtree(out_dir)
    shutil.copytree(template_dir, out_dir)

    # Every member of a save is prefixed with the save's own name, and the game
    # ignores an archive whose members disagree with it - renaming the folder alone
    # produces a main menu with Load Game greyed out.
    for root, _, files in os.walk(out_dir):
        for filename in files:
            if filename.startswith(TEMPLATE_NAME):
                os.rename(
                    os.path.join(root, filename),
                    os.path.join(root, name + filename[len(TEMPLATE_NAME) :]),
                )
    for root, dirs, _ in os.walk(out_dir, topdown=False):
        for dirname in dirs:
            if dirname.startswith(TEMPLATE_NAME):
                os.rename(
                    os.path.join(root, dirname),
                    os.path.join(root, name + dirname[len(TEMPLATE_NAME) :]),
                )

    first = os.path.join(out_dir, f"{name}.1st.ntwtf.json")
    second = os.path.join(out_dir, f"{name}.2nd.ntwtf.json")
    parts = os.path.join(out_dir, f"{name}.ntwtf.lua.parts")

    preload = load(first)
    preload["areaId"] = AREA_ID
    save(first, preload)

    body = load(second)
    body["playerCharacter"]["Money"] = money
    save(second, body)

    actors = load(os.path.join(parts, "Actor.json"))
    actors[player_actor(actors)][POSITION_FIELD] = STAND_AT
    save(os.path.join(parts, "Actor.json"), actors)

    variables = load(os.path.join(parts, "Variable.json"))
    variables.update(VARIABLES)
    save(os.path.join(parts, "Variable.json"), variables)

    return out_dir


def build_global_state(index_path, out_path):
    """Marks every entry of the conversation as seen elsewhere, bar the one."""
    conversation = None
    with io.open(index_path, encoding="utf-8") as handle:
        for line in handle:
            row = json.loads(line)
            if row["id"] == CONVERSATION:
                conversation = row
                break

    if conversation is None:
        raise ValueError(
            f"Conversation {CONVERSATION} is not in {index_path}. Run extract_conversation_index.py first."
        )

    marked = sorted(entry["id"] for entry in conversation["entries"] if entry["id"] != UNSEEN_ENTRY)
    if UNSEEN_ENTRY not in {entry["id"] for entry in conversation["entries"]}:
        raise ValueError(f"Entry {UNSEEN_ENTRY} is not in conversation {CONVERSATION}")

    state = {
        # GlobalStateJson.FormatVersion. The loader refuses a file without it and
        # refuses one claiming a version it does not know.
        "version": GLOBAL_STATE_VERSION,
        "conversations": {str(CONVERSATION): {str(entry): "WasDisplayed" for entry in marked}},
    }
    save(out_path, state)
    return len(marked)


def build_all(template_dir, index_path, out_root):
    os.makedirs(out_root, exist_ok=True)
    made = []
    for name, money in sorted(SCENARIOS.items()):
        out_dir = os.path.join(out_root, f"{name}.ntwtf")
        build_save(template_dir, out_dir, name, money)
        made.append(f"{name}: money {money}")
        print(f"  {name}.ntwtf  money={money}")

    state_path = os.path.join(out_root, "global-conversation-state.json")
    marked = build_global_state(index_path, state_path)
    print(
        "  global-conversation-state.json  "
        f"{marked} entries of conversation {CONVERSATION} marked, "
        f"entry {UNSEEN_ENTRY} left unseen"
    )
    return made


###############################################################################
# CLI
###############################################################################

DEFAULT_TEMPLATE = os.path.join("testing", f"{TEMPLATE_NAME}.ntwtf")
DEFAULT_INDEX = os.path.join(".game_reference_copies", "derived", "conversation_index.jsonl")
DEFAULT_OUT = os.path.join("testing", "scenarios")


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--template", default=DEFAULT_TEMPLATE, help="The expanded template save")
    parser.add_argument("--index", default=DEFAULT_INDEX, help="The extracted conversation index")
    parser.add_argument("--out", default=DEFAULT_OUT, help="Where to write the scenarios")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        build_all(args.template, args.index, args.out)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
