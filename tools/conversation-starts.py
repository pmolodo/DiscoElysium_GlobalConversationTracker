#!/usr/bin/env python
# run-log-kind: analysis

"""Where every conversation in the game can be started, and what nothing explains.

## Why this is not a search for field names

A conversation starts when something calls `DialogueManager.StartConversation(title)`. The
title is nearly always a serialized field on a component, and the field is called different
things on different components - `conversation` on `BasicEntity` and `SenseOrb`,
`conversationIfNoEntity` on `InteractionAreaTrigger`, `equipOrbName` on `InventoryItem`. A
scan for one spelling silently misses the rest, and reports a confident number.

So components are identified by WHAT THEY ARE. Every MonoBehaviour in the exported scene,
prefab and asset data is read as Unity YAML, its class resolved through its `m_Script` GUID
against the exported `.cs.meta` files, and then every one of its fields is checked against
the conversation titles the index declares. A title sitting in a field nobody anticipated is
found anyway, and the report names the field it was found in.

## The other three sources

CODE, for names that are not on any component: `HardcodedDialogues.LIST` - a static table the
dreams, wakeups and cutscene situations read - plus the three string literals elsewhere in the
game, plus `ReputationAlterant.orbDialogues`, which names one conversation per reputation and
is how the political thoughts arrive. The Final Cut export drops both tables' contents; the
Cpp2IL ISIL dump holds them and agrees with the pre-Final-Cut export, so the strings are
transcribed here.

ITEMS, from the dialogue database's own items table, which names conversations in its
`conversation`, `equipOrb` and `alternativeEquipOrb` fields.

LINKS, which are not starts. A cross-conversation link ENTERS a conversation that is already
running, so a conversation reachable only by link is never started by anything.

## What the residue means, and why the corpus column is here

A conversation with content that nothing starts and nothing links into is either dead or
started by a mechanism this does not model. The save corpus decides it in one direction only:
a conversation a real playthrough displayed was reached somehow, so its presence proves a
missed mechanism, while absence proves nothing - four playthroughs are not exhaustive.

At the time of writing 17 of the 67 residue conversations had been displayed, nearly all of
them `THOUGHT / ...`. Their titles are resolved by the Thought Cabinet at runtime from data
no exported table carries: the items table does not name them, `GainThought` ids match only
11 of 18 by spelling, and the political thoughts are awarded in C# rather than by Lua. That
is the known hole, and the residue column is how it stays visible.
"""

import argparse
import json
import re
import sys
import traceback

from collections import defaultdict, deque
from pathlib import Path

###############################################################################
# Core functions
###############################################################################

EXPORT = Path(".game_reference_copies/AssetRipperExport/ExportedProject/Assets")
INDEX = Path(".game_reference_copies/derived/conversation_index.jsonl")
DATABASE = EXPORT / "Dialogue Databases/Disco Elysium.asset"
CORPUS = Path(r"D:\Downloads\Apps\Games\Disco Elysium\Saves")
OUT = Path("analysis/outputs/conversation-starts.tsv")

SCRIPT_RE = re.compile(r"m_Script:\s*\{fileID:\s*-?\d+,\s*guid:\s*([0-9a-f]{32})")
#: A field of a MonoBehaviour, AT ANY DEPTH. Unity writes a component's own fields at two
#: spaces and the members of a list element deeper than that, with the first member of each
#: element carrying the `- `. Matching only the two-space form reads a component's own fields
#: and silently skips every list of structs it holds - which is where `overrideConversation`
#: lives on a scheduled entity, and is how `APT / WCW MAIN` sat unexplained in a scene that
#: names it.
#:
#: THE SAME MISTAKE THE MODULE DOC WARNS ABOUT, one level along: a scan fixed to one SHAPE
#: reports a confident number, whether what it fixed is the field's spelling or its depth.
FIELD_RE = re.compile(r"^\s+-?\s*([A-Za-z_][A-Za-z0-9_]*):\s*(.*)$")
GUID_RE = re.compile(r"^guid:\s*([0-9a-f]{32})\s*$", re.M)
YAML_SUFFIXES = (".unity", ".prefab", ".asset")

#: `HardcodedDialogues.LIST`, which the Final Cut export declares and leaves empty. Taken
#: from the ISIL dump of the shipped assembly, where all sixteen survive.
HARDCODED = (
    "LIFELINE / GIVING UP",
    "LIFELINE / HEART ATTACK",
    "WHIRLING / DREAM1",
    "LEDGER WAKEUP",
    "WHIRLING F2 / KIM WAKEUP",
    "WHIRLING F2 / CUNO WAKEUP",
    "WHIRLING F2 / DREAM 2 INTRO",
    "WHIRLING F2 / DREAM 2 HANGED MAN",
    "DREAM SEAFORT / NO DOLORES DREAM",
    "SEAFORT INT / AFTERDOLORES TALK",
    "WHIRLING F2 / DREAM 3",
    "WHIRLING F2 / DREAM 4",
    "WHIRLING F2 / DREAM 5",
    "WHIRLING F2 / BLACKOUT DREAM",
    "WHIRLING / KIM MAIN",
    "BACKYARD / KIM APT DOOR barks",
)

#: The only conversation names written as string literals anywhere else in the game's code.
LITERALS = ("ICE / KIM RACISM FINAL TALK", "Stage directions test dialogue")

#: `ReputationAlterant.orbDialogues`, one title per reputation. Crossing a reputation's
#: threshold adds an ORB rather than starting a conversation directly -
#: `GlobalOrbManager.AddObsession(orbDialogues[(int)rep])` - and the orb's conversation is
#: what the player then reaches, which is why these are starts and not links.
#:
#: WHAT AWARDS THEM IS THE TALLY. The political thoughts have no Lua call and no component
#: field, because they are earned by counting dialogue choices in C#; this array is where that
#: counting names its conversations.
#:
#: The Final Cut export declares the array and strips its initialiser, exactly as it does
#: `HardcodedDialogues.LIST`, so these are transcribed from the pre-Final-Cut export whose
#: bodies decompile. Confirm against the ISIL dump before treating the list as exact.
ORB_DIALOGUES = (
    "THOUGHT / APOCALYPSE COP",
    "THOUGHT / BORING COP",
    "THOUGHT / SUPERSTAR COP",
    "THOUGHT / SORRY COP",
    "THOUGHT / WORLD REPUBLIC",
    "THOUGHT / REVACHOLIAN NATIONHOOD",
    "THOUGHT / GOSSAMER STATE",
    "THOUGHT / KINGDOM OF CONSCIENCE",
    "THOUGHT / HONOUR",
    "THOUGHT / THE DESTROYER",
    "THOUGHT / TORQUE DORK",
    "THOUGHT / COACH PHYSICAL INSTRUMENT",
    "THOUGHT / ART COP",
    "THOUGHT / REMOTE VIEWER",
    "THOUGHT / SUICIDE COP",
)


def load_index(path):
    """Each conversation's entries and title, by id."""
    entries, titles = {}, {}
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            row = json.loads(line)
            entries[row["id"]] = {e["id"]: e for e in row["entries"]}
            titles[row["id"]] = row["title"]
    return entries, titles


def content_from(index, start):
    """Non-group entries a link walk from `start` reaches, following cross-conversation links."""
    seen, queue = set(), deque([start])
    while queue:
        conversation, entry = queue.popleft()
        node = index.get(conversation, {}).get(entry)
        if node is None:
            continue
        targets = node.get("to") or []
        elsewhere = node.get("to_conversation") or []
        for position, target in enumerate(targets):
            destination = elsewhere[position] if position < len(elsewhere) else conversation
            if (destination, target) not in seen:
                seen.add((destination, target))
                queue.append((destination, target))
    seen.discard(start)
    return sum(1 for c, e in seen if not index.get(c, {}).get(e, {}).get("group", False))


def linked_into(index):
    """Every conversation some OTHER conversation links into."""
    found = set()
    for conversation, entries in index.items():
        for entry in entries.values():
            targets = entry.get("to") or []
            elsewhere = entry.get("to_conversation") or []
            for position, _target in enumerate(targets):
                destination = elsewhere[position] if position < len(elsewhere) else conversation
                if destination != conversation:
                    found.add(destination)
    return found


def script_guids(export):
    """GUID -> class name, from every exported script's `.meta`."""
    found = {}
    for meta in export.rglob("*.cs.meta"):
        try:
            guid = GUID_RE.search(meta.read_text(encoding="utf-8", errors="replace"))
        except OSError:
            continue
        if guid:
            found[guid.group(1)] = meta.name[: -len(".cs.meta")]
    return found


def yaml_documents(path):
    """Each Unity YAML document in `path`, as its lines."""
    current = None
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("--- !u!"):
            if current is not None:
                yield current
            current = []
        elif current is not None:
            current.append(line)
    if current is not None:
        yield current


#: Assets that LIST conversation titles as their data rather than naming one to start.
#:
#: The dialogue database holds every title in the game, and a lockit holds every one again per
#: language. Reading either as a component makes all 1,501 conversations look startable and the
#: residue vanish - which is the answer this tool exists to refuse to give. The database is read
#: properly by `named_by_items`, against its items table alone.
DATA_TABLES = ("Dialogue Databases", "lockits", "VoiceOverClipsLibrary", "path_id_map")


def is_a_data_table(path):
    return any(part in str(path) for part in DATA_TABLES)


def named_by_components(export, titles, guids):
    """(class, field) -> the conversation titles it holds, over all exported YAML."""
    holders = defaultdict(set)
    for path in export.rglob("*"):
        if path.suffix not in YAML_SUFFIXES or not path.is_file():
            continue
        if is_a_data_table(path):
            continue
        try:
            documents = list(yaml_documents(path))
        except OSError:
            continue
        for document in documents:
            script = SCRIPT_RE.search("\n".join(document[:14]))
            if not script:
                continue
            owner = guids.get(script.group(1), f"guid:{script.group(1)}")
            for line in document:
                field = FIELD_RE.match(line)
                if not field:
                    continue
                value = field.group(2).strip().strip("'\"")
                if value in titles:
                    holders[(owner, field.group(1))].add(value)
                    continue
                # A PREFIX IS A RULE RATHER THAN A NAME, and it starts every conversation it
                # matches. `CreateOrbs` walks the whole database and builds a SenseOrb for each
                # title beginning with its `LocationPrefix`, so the field holds no conversation
                # name at all and an exact match finds it only by accident - which is why
                # `WHIRLING F2 ORB / speed hangover` read as started by nothing while 689 of 927
                # saves had displayed it.
                if owner == "CreateOrbs" and field.group(1) == "LocationPrefix" and len(value) >= 5:
                    holders[(owner, field.group(1))].update(title for title in titles if title.startswith(value))
    return holders


def named_by_items(database, titles, span=(14789, 36012)):
    """Conversation titles the dialogue database's items table names."""
    found, field = set(), None
    if not Path(database).exists():
        return found
    with open(database, encoding="utf-8", errors="replace") as handle:
        for number, line in enumerate(handle, 1):
            if number < span[0]:
                continue
            if number > span[1]:
                break
            text = line.strip()
            if text.startswith("- title: "):
                field = text[len("- title: ") :]
            elif text.startswith("value: ") and field:
                value = text[len("value: ") :].strip().strip("'\"")
                if value in titles:
                    found.add(value)
    return found


def ids_in(spec):
    """The ids a "0,3-4,12" range string names."""
    found = set()
    for piece in (p for p in (spec or "").split(",") if p):
        if "-" in piece:
            first, last = piece.split("-", 1)
            found.update(range(int(first), int(last) + 1))
        else:
            found.add(int(piece))
    return found


def corpus_counts(corpus):
    """conversation -> how many saves displayed any of its entries, and how many were read.

    An EMPTY corpus means none, and has to be checked for by hand: `Path("")` is `Path(".")`,
    which exists, so the obvious test passes and the walk descends into whatever directory the
    tool happens to be run from. This repository's own test saves are Conversation.json files
    of exactly the right shape, so the run produces a plausible smaller number rather than an
    error - a corpus column silently describing the fixtures instead of real playthroughs.
    """
    seen, files = defaultdict(int), 0
    if not str(corpus).strip():
        return seen, files
    root = Path(corpus)
    if not root.exists():
        return seen, files
    for path in sorted(root.rglob("Conversation.json")):
        if "lua.parts" not in str(path):
            continue
        files += 1
        with open(path, encoding="utf-8") as handle:
            changes = json.load(handle).get("_changes", {})
        for conversation, body in changes.items():
            if ids_in((body.get("Dialog") or {}).get("WasDisplayed")):
                seen[int(conversation)] += 1
    return seen, files


def write_dataset(index, titles, sources, out):
    """One row per conversation; returns the counts worth printing."""
    component, code, item, links, saves = sources
    by_title = defaultdict(set)
    for conversation, title in titles.items():
        by_title[title].add(conversation)

    def ids_of(names):
        found = set()
        for name in names:
            found |= by_title.get(name, set())
        return found

    from_component = ids_of(component)
    from_code = ids_of(code)
    from_item = ids_of(item)

    out = Path(out)
    out.parent.mkdir(parents=True, exist_ok=True)
    counts = {"startable": 0, "reachable": 0, "residue": 0, "residue_displayed": 0}
    with open(out, "w", encoding="utf-8", newline="") as handle:
        handle.write(
            "conversation\tentries\tcontent_entry0\tby_component\tby_code\tby_item\t"
            "linked_into\tsaves_displayed\tstartable\treachable\tresidue\ttitle\n"
        )
        for conversation in sorted(index):
            content = content_from(index, (conversation, 0)) if 0 in index[conversation] else 0
            startable = conversation in from_component or conversation in from_code or conversation in from_item
            reachable = startable or conversation in links
            residue = int(content > 0 and not reachable)
            counts["startable"] += int(startable)
            counts["reachable"] += int(reachable)
            counts["residue"] += residue
            counts["residue_displayed"] += int(bool(residue and saves.get(conversation)))
            handle.write(
                f"{conversation}\t{len(index[conversation])}\t{content}\t"
                f"{int(conversation in from_component)}\t{int(conversation in from_code)}\t"
                f"{int(conversation in from_item)}\t{int(conversation in links)}\t"
                f"{saves.get(conversation, 0)}\t{int(startable)}\t{int(reachable)}\t"
                f"{residue}\t{titles[conversation]}\n"
            )
    return counts


def build(export=EXPORT, index_path=INDEX, database=DATABASE, corpus=CORPUS, out=OUT):
    export = Path(export)
    index, titles = load_index(index_path)
    every_title = set(titles.values())
    print(f"conversations                {len(index)}")

    guids = script_guids(export)
    print(f"scripts with a GUID          {len(guids)}")

    holders = named_by_components(export, every_title, guids)
    print("\ncomponent types holding conversation names:")
    for (owner, field), names in sorted(holders.items(), key=lambda kv: -len(kv[1])):
        print(f"  {owner:32} .{field:26} {len(names):5}")
    component = {name for names in holders.values() for name in names}

    item = named_by_items(database, every_title)
    print(f"\nnamed by the items table     {len(item)}")

    links = linked_into(index)
    saves, files = corpus_counts(corpus)
    print(f"linked into from elsewhere   {len(links)}")
    print(f"saves scanned                {files}")

    counts = write_dataset(index, titles, (component, HARDCODED + LITERALS + ORB_DIALOGUES, item, links, saves), out)
    print(f"\nstartable                    {counts['startable']}")
    print(f"reachable (starts + links)   {counts['reachable']}")
    print(f"residue                      {counts['residue']}")
    print(f"  displayed in a real save   {counts['residue_displayed']}")
    print(f"\nwritten to {Path(out).resolve()}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--export", default=str(EXPORT), help="the AssetRipper export")
    parser.add_argument("--index", default=str(INDEX), help="the conversation index")
    parser.add_argument("--database", default=str(DATABASE), help="the dialogue database")
    parser.add_argument("--corpus", default=str(CORPUS), help="the real-save corpus")
    parser.add_argument("--out", default=str(OUT), help="where to write the dataset")
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        build(args.export, args.index, args.database, args.corpus, args.out)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
