#!/usr/bin/env python

"""Which dialogue actions write something a guard downstream of them reads.

For every function a userScript calls, this finds the state the game's body writes - as
`WRITES` below records it, from the game's own source - and asks whether any guard REACHABLE
from the entry carrying the call reads that state. An action whose writes no downstream guard
can observe cannot change which branch a crawl takes, whatever it does to the game.

## What downstream means

Reachable along dialogue links from the entry that carries the script, following links into
other conversations, and IGNORING guards: a link counts whether or not its guard could pass.
That over-approximates what a crawl can walk, so "no downstream reader" is a sound exclusion
and "a downstream reader" is a candidate rather than a proof. The carrying entry's own guard
does not count, since it is decided before its script runs, unless a link cycle leads back to
it.

## What a key is

State is named by a key, and a writer and a reader meet when they name the same key:

    var:<name>        a dialogue variable, Variable["name"]
    item:<name>       whether an item is held - CheckItem
    task:<name>       whether a journal task or subtask is active - IsTaskActive
    thought:<name>    whether a thought is gained - IsTHCPresent
    group:<name>      whether anything in an item group is held - CheckItemGroup
    equipment         what is worn or held - CheckEquipped and the clothing questions
    tab               what the inventory tabs hold - HasPawnablesInInventory
    cabinet           which thoughts are cooking or fixed
    money, clock, kim, cuno, volition, endurance, scene

The journal and item tables are read from the raw dialogue database asset, because the derived
index does not carry conversation fields or item properties.

Usage:
    tools/survey-action-readers.py                 # the per-function table
    tools/survey-action-readers.py --detail GainItem
"""

import argparse
import collections
import json
import re
import sys
import traceback

DEFAULT_DERIVED = ".game_reference_copies/derived"
DEFAULT_ASSET = ".game_reference_copies/AssetRipperExport/ExportedProject/Assets/Dialogue Databases/Disco Elysium.asset"

LUA_KEYWORDS = {"and", "or", "not", "if", "then", "return", "function", "end", "once"}
NAME = re.compile(r"[A-Za-z_]\w*")
VARIABLE_READ = re.compile(r'Variable\s*\[\s*"([^"]+)"\s*\]')
DIRECT_ASSIGNMENT = re.compile(r'Variable\s*\[\s*"([^"]+)"\s*\]\s*=(?!=)\s*([^;\n]*)')
BLOCK_COMMENT = re.compile(r"--\[\[.*?\]\]", re.DOTALL)

REPUTATION_RANGES = {
    "IsHighestCopotype": ["art_cop", "superstar_cop", "apocalypse_cop", "sorry_cop"],
    "IsHighestPolitical": ["communist", "ultraliberal", "moralist", "revacholian_nationhood"],
}
CLOCK_READERS = {
    "DayCount",
    "HourCount",
    "TotalHourCount",
    "IsHour",
    "IsHourBetween",
    "IsMorning",
    "IsAfternoon",
    "IsEvening",
    "IsNight",
    "IsNighttime",
    "IsDaytime",
    "IsNoon",
    "IsDusk",
    "IsMidnight",
    "IsDayFrom",
    "IsDayUntil",
}
EQUIPMENT_READERS = {
    "CheckEquipped",
    "CheckEquippedGroup",
    "CheckHeldLeftGroup",
    "CheckHeldRightGroup",
    "HasHat",
    "HasJacket",
    "HasNecktie",
    "HasPants",
    "HasShirt",
    "HasShoes",
    "WeirdClothing",
}
KIM_READERS = {"IsKimHere", "IsKimInParty"}
TASK_PARTS = ("display", "done", "cancel")

# fmt: off
# What each script function writes, as keys. `{0}` is the first argument. The game source each
# row rests on is quoted in docs/actions.md; rows marked COMPUTED are expanded in `writes_of`.
WRITES = {
    "SetVariableValue": ["var:{0}"],
    "SetFlag": ["var:{0}"],
    "UnsetFlag": ["var:{0}"],
    "XPPicoSetBool": ["var:{0}"],
    "XPTinySetBool": ["var:{0}"],
    "XPMinorSetBool": ["var:{0}"],
    "XPStandardSetBool": ["var:{0}"],
    "XPMajorSetBool": ["var:{0}"],
    "ReputationGrows": ["var:reputation.{0}"],
    "ReputationLowers": ["var:reputation.{0}"],
    "Reputation": ["var:reputation.{0}"],
    "GainTask": "COMPUTED",
    "FinishTask": "COMPUTED",
    "CancelTask": "COMPUTED",
    "GainItem": "COMPUTED",
    "LoseItem": "COMPUTED",
    "SellItemGroup": "COMPUTED",
    "SellItemGroupWithModifier": "COMPUTED",
    "GainThought": ["thought:{0}"],
    "GainMoneyOnce": ["money"],
    "GainMoneyAlways": ["money"],
    "LoseMoneyOnce": ["money"],
    "LoseMoneyAlways": ["money"],
    "PassTime": ["clock", "cabinet"],
    "UseSubstanceInHand": "COMPUTED",
    "DamageVolition": ["volition"],
    "HealVolition": ["volition"],
    "HealAllVolition": ["volition"],
    "DamageEndurance": ["endurance"],
    "HealEndurance": ["endurance"],
    "DamageEnduranceWithNewspaper": ["endurance"],
    "ReturnKitsuragi": ["kim"],
    "RemoveAndHideKitsuragi": ["kim"],
    "RemoveAndHideKitsuragiUntilMorning": ["kim"],
    "RemoveKitsuragiWaitAtChurch": ["kim"],
    "RemoveKitsuragiWaitAtLair": ["kim"],
    "RemoveKitsuragiWaitAtTent": ["kim"],
    "NightyNightKitsuragiShack": ["kim"],
    "AddCunoToParty": ["cuno"],
    "RemoveCunoFromParty": ["cuno"],
    "RemoveCunoWaitAtFort": ["cuno"],
    "GoTo": ["scene"],
    "GoToDestination": ["scene"],
}
# fmt: on

###############################################################################
# Reading the database
###############################################################################


def asset_objects(path):
    """Each object in the database asset, as its field titles mapped to their values."""
    current = {}
    title = None
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            stripped = line.strip()
            if stripped.startswith("- title: "):
                title = stripped[len("- title: ") :]
                continue
            if title is None or not stripped.startswith("value:"):
                continue
            value = stripped[len("value:") :].strip()
            if title in ("Title", "Articy Id") and title in current:
                yield current
                current = {}
            current[title] = value
            title = None
    if current:
        yield current


def journal_and_items(asset):
    """The journal's parts by variable name, and every item's properties by name."""
    parts = {}
    items = {}
    for fields in asset_objects(asset):
        if fields.get("IsItem") == "True" and "autoequip" in fields:
            items[fields.get("Name")] = fields
        if "display_condition_main" not in fields:
            continue
        main = part_of(fields, "condition_main")
        subtasks = [part_of(fields, f"subtask_{i:02}") for i in range(1, 13)]
        subtasks = [sub for sub in subtasks if sub["display"]]
        for part in [main] + subtasks:
            part["task"] = main["display"]
            part["subtasks"] = [sub["display"] for sub in subtasks] if part is main else []
            for kind in TASK_PARTS:
                if part[kind]:
                    parts[part[kind]] = part
    return parts, items


def part_of(fields, suffix):
    def variable(kind):
        found = VARIABLE_READ.findall(fields.get(f"{kind}_{suffix}", ""))
        return found[0] if found else None

    return {kind: variable(kind) for kind in TASK_PARTS}


def read_index(derived):
    """Every entry's guard and script, and the link graph between entries."""
    nodes = {}
    edges = collections.defaultdict(list)
    with open(f"{derived}/conversation_index.jsonl", encoding="utf-8") as handle:
        for line in handle:
            record = json.loads(line)
            for entry in record["entries"]:
                node = (record["id"], entry["id"])
                nodes[node] = (entry.get("guard") or "", entry.get("script") or "")
                targets = entry.get("to", [])
                conversations = entry.get("to_conversation") or [record["id"]] * len(targets)
                for target, conversation in zip(targets, conversations):
                    edges[node].append((conversation, target))
    return nodes, edges


###############################################################################
# Reading Lua
###############################################################################


def normalise(text):
    """Comments stripped and the two-character statement separator turned into a newline."""
    text = BLOCK_COMMENT.sub(" ", text)
    out = []
    i = 0
    in_string = False
    while i < len(text):
        c = text[i]
        if in_string:
            out.append(c)
            if c == "\\" and i + 1 < len(text):
                out.append(text[i + 1])
                i += 1
            elif c == '"':
                in_string = False
        elif c == '"':
            in_string = True
            out.append(c)
        elif c == "\\" and text[i + 1 : i + 2] == "n":
            out.append("\n")
            i += 1
        else:
            out.append(c)
        i += 1
    return "".join(out)


def calls(text):
    """Every call in `text`, as its name and its top-level argument texts."""
    found = []
    i = 0
    while i < len(text):
        if text[i] == '"':
            i = skip_string(text, i)
            continue
        match = NAME.match(text, i)
        if not match or (i > 0 and (text[i - 1].isalnum() or text[i - 1] in "_.")):
            i += 1
            continue
        j = match.end()
        while j < len(text) and text[j].isspace():
            j += 1
        if j >= len(text) or text[j] != "(" or match.group() in LUA_KEYWORDS:
            i = match.end()
            continue
        args, end = arguments(text, j)
        found.append((match.group(), args))
        i = j + 1
    return found


def skip_string(text, i):
    i += 1
    while i < len(text) and text[i] != '"':
        i += 2 if text[i] == "\\" else 1
    return i + 1


def arguments(text, open_at):
    depth = 0
    current = []
    args = []
    i = open_at
    while i < len(text):
        c = text[i]
        if c == '"':
            end = skip_string(text, i)
            current.append(text[i:end])
            i = end
            continue
        if c == "(":
            depth += 1
            if depth > 1:
                current.append(c)
        elif c == ")":
            depth -= 1
            if depth == 0:
                break
            current.append(c)
        elif c == "," and depth == 1:
            args.append("".join(current).strip())
            current = []
        else:
            current.append(c)
        i += 1
    last = "".join(current).strip()
    if last:
        args.append(last)
    return args, i


def unquote(arg):
    arg = arg.strip()
    return arg[1:-1] if len(arg) >= 2 and arg[0] == arg[-1] == '"' else arg


###############################################################################
# Keys
###############################################################################


def reads_of(guard, parts, items):
    """The keys a guard reads."""
    keys = {f"var:{name}" for name in VARIABLE_READ.findall(guard)}
    for name, args in calls(normalise(guard)):
        first = unquote(args[0]) if args else ""
        if name == "CheckItem":
            keys.add(f"item:{first}")
        elif name == "IsTaskActive":
            keys.add(f"task:{first}")
        elif name == "IsTHCPresent":
            keys.add(f"thought:{first}")
        elif name == "CheckItemGroup":
            keys.add(f"group:{first}")
        elif name in ("FlagSet", "FlagNotSet"):
            keys.add(f"var:{first}")
        elif name in ("SubstanceUsedOnce", "SubstanceUsedMore"):
            keys.add(f"var:stats.uses_{first}")
        elif name in ("IsRaining", "IsSnowing"):
            keys.add("var:auto.is_raining" if name == "IsRaining" else "var:auto.is_snowing")
        elif name in REPUTATION_RANGES:
            keys.update(f"var:reputation.{rep}" for rep in REPUTATION_RANGES[name])
        elif name in EQUIPMENT_READERS:
            keys.add("equipment")
        elif name == "HasPawnablesInInventory":
            keys.add("tab")
        elif name in ("IsTHCCooking", "IsTHCFixed", "IsTHCCookingOrFixed"):
            keys.add("cabinet")
        elif name == "MoneyAmount":
            keys.add("money")
        elif name in CLOCK_READERS:
            keys.add("clock")
        elif name in KIM_READERS:
            keys.add("kim")
        elif name == "IsCunoInParty":
            keys.add("cuno")
        elif name == "HasVolitionDamage":
            keys.add("volition")
        elif name == "HasEnduranceDamage":
            keys.add("endurance")
        elif name == "IsExterior":
            keys.add("scene")
    return keys


def item_writes(item, props, losing):
    """What gaining or losing one item can move.

    Gaining equips only an item flagged `autoequip` (`Inventory.HandlePickedUpItem`). Losing
    unequips whatever it deletes (`Inventory.DeleteItem`), so losing counts as writing
    equipment for any item that can be worn or held at all.
    """
    keys = {f"item:{item}", "tab"}
    if props is None or props.get("autoequip") == "True" or (losing and equippable(props)):
        keys.add("equipment")
    if props is None:
        return keys | {"money"}
    group = ITEM_GROUPS.get(props.get("itemGroup", "0"))
    if group and group != "none":
        keys.add(f"group:{group}")
    if props.get("isConsumable") == "True" and props.get("itemValue", "0") not in ("", "0"):
        keys.add("money")
    return keys


def equippable(props):
    """Whether an item goes in an equipment slot.

    The database's `itemType` is the game's `ItemType` enum plus one - `shirt_*` items are 2
    where `SHIRT` is 1, `HELD` things like the prybar are 11 where `HELD` is 10 - so ARMOR to
    HELD are 1 to 11, and keys and papers (13) go in no slot.
    """
    return props.get("itemType", "").isdigit() and 1 <= int(props["itemType"]) <= EQUIPPABLE_TYPES


EQUIPPABLE_TYPES = 11

ITEM_GROUPS = dict(enumerate(["none", "alcohol", "smokes", "ghb", "speed", "pyrholidon", "tare"]))
ITEM_GROUPS = {str(k): v for k, v in ITEM_GROUPS.items()}


def task_writes(name, first, parts):
    """What a journal action writes: its variable, and the activity of its task and subtasks."""
    part = parts.get(first)
    if part is None:
        return {f"var:{first}", f"task:{first}"}, "unresolved"
    written = {"GainTask": "display", "FinishTask": "done", "CancelTask": "cancel"}[name]
    keys = {f"task:{part['display']}"} | {f"task:{sub}" for sub in part["subtasks"]}
    if part[written]:
        keys.add(f"var:{part[written]}")
    if name == "FinishTask" and part["display"]:
        keys.add(f"var:{part['display']}")
    return keys, "argument names the " + ("same" if first == part[written] else "other") + " part"


def writes_of(name, args, parts, items):
    """The keys one call writes, and a note on how they were resolved."""
    first = unquote(args[0]) if args else ""
    rule = WRITES.get(name)
    if rule is None:
        return set(), None
    if rule != "COMPUTED":
        return {key.format(first) for key in rule}, None
    if name in ("GainTask", "FinishTask", "CancelTask"):
        return task_writes(name, first, parts)
    if name in ("GainItem", "LoseItem"):
        note = f"itemType {items[first].get('itemType')}" if first in items else "unknown item"
        return item_writes(first, items.get(first), name == "LoseItem"), note
    if name in ("SellItemGroup", "SellItemGroupWithModifier"):
        members = {n for n, p in items.items() if ITEM_GROUPS.get(p.get("itemGroup")) == first}
        return {f"group:{first}", "money", "tab"} | {f"item:{m}" for m in members}, None
    if name == "UseSubstanceInHand":
        uses = {f"var:stats.uses_{g}" for g in ITEM_GROUPS.values() if g not in ("none", "tare")}
        return uses | {"equipment"}, None
    raise AssertionError(name)


INCREMENT = re.compile(r'Variable\s*\[\s*"([^"]+)"\s*\]\s*[+-]\s*(once\s*\(\s*)?-?\d+\s*\)?')


def assigned_kind(args):
    """What shape of value a SetVariableValue assigns: literal, increment, text or computed."""
    value = args[1].strip() if len(args) > 1 else ""
    if value in ("true", "false") or re.fullmatch(r"-?\d+(\.\d+)?", value):
        return "literal"
    if value.startswith('"'):
        return "text"
    if INCREMENT.fullmatch(value):
        return "increment"
    return "computed"


###############################################################################
# Survey
###############################################################################


def reversed_links(edges):
    reverse = collections.defaultdict(list)
    for source, targets in edges.items():
        for target in targets:
            reverse[target].append(source)
    return reverse


def reaching(readers, reverse):
    """Every node with a path of at least one link to a node in `readers`."""
    seen = set()
    stack = list(readers)
    while stack:
        node = stack.pop()
        for source in reverse.get(node, ()):
            if source not in seen:
                seen.add(source)
                stack.append(source)
    return seen


def survey(derived, asset, detail):
    parts, items = journal_and_items(asset)
    nodes, edges = read_index(derived)

    readers = collections.defaultdict(set)
    for node, (guard, _) in nodes.items():
        for key in reads_of(guard, parts, items):
            readers[key].add(node)

    instances = collections.defaultdict(list)
    for node, (_, script) in nodes.items():
        text = normalise(script)
        for name, args in calls(text):
            keys, note = writes_of(name, args, parts, items)
            instances[name].append((node, args, keys, note))
        for variable, value in DIRECT_ASSIGNMENT.findall(text):
            instances["<Variable assignment>"].append((node, [variable, value], {f"var:{variable}"}, None))

    wanted = {key for rows in instances.values() for _, _, keys, _ in rows for key in keys}
    reverse = reversed_links(edges)
    reach = {key: reaching(readers[key], reverse) for key in wanted if key in readers}

    print(f"journal parts: {len(parts)}, items: {len(items)}, entries: {len(nodes)}\n")
    header = f"{'function':<36} {'calls':>6} {'live':>6}  keys read downstream"
    print(header)
    for name in sorted(instances, key=lambda n: (-len(instances[n]), n)):
        rows = instances[name]
        live = [(node, args, [k for k in keys if node in reach.get(k, ())]) for node, args, keys, _ in rows]
        live = [row for row in live if row[2]]
        families = collections.Counter(k.split(":")[0] for _, _, ks in live for k in ks)
        described = ", ".join(f"{f} x{n}" for f, n in families.most_common())
        known = "" if name in WRITES or name == "<Variable assignment>" else "  (no writes recorded)"
        print(f"{name:<36} {len(rows):>6} {len(live):>6}  {described}{known}")
        if name == "SetVariableValue":
            kinds = collections.Counter(assigned_kind(args) for _, args, _ in live)
            print(f"    live by value: {dict(kinds)}")
            for node, args, _ in live:
                if assigned_kind(args) in ("computed", "text"):
                    print(f"      {node[0]}:{node[1]} {args}")
        if name == detail:
            notes = collections.Counter(note for _, _, _, note in rows)
            print(f"    notes: {dict(notes)}")
            for node, args, keys in live[:40]:
                print(f"    {node[0]}:{node[1]} {args} -> {sorted(keys)}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--derived", default=DEFAULT_DERIVED, help="the extractor's derived folder")
    parser.add_argument("--asset", default=DEFAULT_ASSET, help="the raw dialogue database asset")
    parser.add_argument("--detail", help="list the live instances of one function")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        survey(args.derived, args.asset, args.detail)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
