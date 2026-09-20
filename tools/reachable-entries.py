#!/usr/bin/env python
# run-log-kind: analysis

"""Which entries no conversation start reaches, walking links and ignoring guards.

## The question this answers, and the two it does not

WALK FROM ENTRY 0 OF EVERY CONVERSATION THE GAME CAN START, union the results, and report
per entry whether anything reached it. An entry in no walk is reachable by no dialogue link
from any start the game offers.

THAT IS NOT "unreachable in game", in either direction, and the distinction is the whole
point of the exercise:

- EDGE-ONLY MEANS GUARDS IGNORED. A guard can refuse a link, it cannot create one, so what
  comes out is a SUPERSET of what a player can reach. The world-conditioned question is a
  different one and has its own answer elsewhere; conflating them is easy because both get
  called "reachable".
- THE ROOT SET IS AN ANSWER, not an axiom. `tools/conversation-starts.py` decides which
  conversations the game can start, by resolving every MonoBehaviour in the exported scene
  and prefab data against the titles the index declares, plus the code tables and the items
  table. `--roots all` treats all 1,501 as startable instead, which is the looser claim, and
  the difference between the two runs is what the root set is worth.

CROSS-CONVERSATION LINKS LAND ON ARBITRARY ENTRIES, which is why this is a walk over one
graph rather than a per-conversation count: conversation 7's entry 8 jumps into the middle of
688. STARTS, by contrast, land only on entry 0 - every construction of `ConversationModel` in
the exported game passes -1 for its initial entry, no sequencer command starts a conversation,
and the shipped database never calls the Lua position stack. That is what makes entry 0 the
root of each walk rather than one root among several.

## The check that the walk is the same walk

`performance/seen_profile.rs` walks ONE group from its own start, and the matrix's numbers
come from it. Before any new number is worth anything, this reproduces two of its: 32
candidates for group 7 and 2,311 for group 16, under the same rules - links paired
positionally with `to_conversation`, a ragged pair staying in the current conversation, and
the start itself and every group entry dropped at the end. `--verify` alone runs just that.

## The index the mod ships holds the same graph

`ShippedIndex.Trim` drops fields and titles; it is one-to-one on conversations and on
entries, and copies `Id`, `Group`, `To` and `ToConversation` verbatim. So an entry cannot be
absent from the shipped index, and the walk cannot differ. This checks it rather than
asserting it, by walking the trimmed index too and comparing the two answers.
"""

import argparse
import csv
import json
import sys
import traceback

from collections import deque
from pathlib import Path

###############################################################################
# Core functions
###############################################################################

INDEX = Path(".game_reference_copies/derived/conversation_index.jsonl")
TRIMMED = Path(".game_reference_copies/derived/conversation_index.trimmed.jsonl")
STARTS = Path("analysis/outputs/conversation-starts.tsv")
OUT = Path("analysis/outputs/reachable-entries.tsv")

#: The two numbers `performance/seen_profile.rs` produces for these groups. Getting them out
#: of this walk is what says the walk is the same walk.
KNOWN_CANDIDATES = {7: 32, 16: 2311}


def read_index(path):
    """Every conversation by id: its title, and its entries by id.

    The shipped index opens with a `{"format": n}` line and the full one does not, so a
    record without entries is that header and nothing else - anything else is an error
    rather than a record to skip past.
    """
    conversations = {}
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            record = json.loads(line)
            if "entries" not in record:
                if set(record) != {"format"}:
                    raise SystemExit(f"{path}: record with no entries: {sorted(record)}")
                continue
            entries = {entry["id"]: entry for entry in record["entries"]}
            conversations[record["id"]] = {
                "title": record.get("title"),
                "entries": entries,
            }
    return conversations


def links_of(conversations, conversation_id, entry):
    """Where an entry's links go, as (conversation, entry) pairs.

    `to_conversation` is paired POSITIONALLY with `to`, and is absent or short exactly when
    the link stays inside the entry's own conversation - so a missing element means "here".
    A link to a node the index does not hold ends the branch, matching the graph builder,
    which drops a link whose destination it cannot resolve.
    """
    destinations = entry.get("to_conversation") or []
    for index, target in enumerate(entry.get("to") or []):
        where = destinations[index] if index < len(destinations) else conversation_id
        conversation = conversations.get(where)
        if conversation is not None and target in conversation["entries"]:
            yield (where, target)


def build_graph(conversations):
    """Every node's links, resolved once, as a dict of (conversation, entry) -> list."""
    graph = {}
    for conversation_id, conversation in conversations.items():
        for entry_id, entry in conversation["entries"].items():
            node = (conversation_id, entry_id)
            graph[node] = list(links_of(conversations, conversation_id, entry))
    return graph


def reached_from(graph, start, within=None):
    """Every node reachable from `start`, following links and ignoring guards.

    `within` restricts the walk to a set of conversations, which is what makes a group's
    walk a group's walk.
    """
    seen = {start}
    queue = deque([start])
    while queue:
        node = queue.popleft()
        for child in graph.get(node, ()):
            if within is not None and child[0] not in within:
                continue
            if child not in seen:
                seen.add(child)
                queue.append(child)
    return seen


def discover_group(conversations, start):
    """The conversations a group spans: the forward closure over `to_conversation`.

    The same rule as `src/index/mod.rs::discover_group`, including that a link to a
    conversation the index does not hold ends the branch rather than being an error.
    """
    group = {start}
    pending = deque([start])
    while pending:
        current = pending.popleft()
        conversation = conversations.get(current)
        if conversation is None:
            continue
        for entry in conversation["entries"].values():
            for destination in entry.get("to_conversation") or []:
                if destination in conversations and destination not in group:
                    group.add(destination)
                    pending.append(destination)
    return group


def candidates_of(conversations, graph, start_conversation):
    """What `performance/seen_profile.rs::candidates` would return for this group.

    GROUPS ARE DROPPED because the game never writes a group's SimStatus, and the start
    itself is dropped because a profile names entries other than where it began.
    """
    group = discover_group(conversations, start_conversation)
    start = (start_conversation, 0)
    reached = reached_from(graph, start, within=group)
    return {node for node in reached if node != start and not conversations[node[0]]["entries"][node[1]].get("group")}


def verify(conversations, graph):
    """Reproduce the matrix's own candidate counts, and say so. Raises if they differ."""
    for conversation_id, expected in sorted(KNOWN_CANDIDATES.items()):
        found = len(candidates_of(conversations, graph, conversation_id))
        mark = "ok" if found == expected else "DIFFERS"
        print(f"  group {conversation_id:<6} candidates {found:>6}  expected {expected:>6}  {mark}")
        if found != expected:
            raise SystemExit(
                f"group {conversation_id} gives {found} candidates, not the {expected} "
                "performance/seen_profile.rs produces - the walk is not the same walk"
            )


def startable_conversations(path):
    """The conversations the game can start, from `tools/conversation-starts.py`."""
    with path.open(encoding="utf-8", newline="") as handle:
        rows = list(csv.DictReader(handle, delimiter="\t"))
    return {int(row["conversation"]) for row in rows if row["startable"] == "1"}


def union_reached(graph, roots):
    """Every node any root reaches, and how many roots reached each."""
    reached = {}
    for root in sorted(roots):
        for node in reached_from(graph, root):
            reached[node] = reached.get(node, 0) + 1
    return reached


def report(conversations, graph, roots, out):
    """Write one row per entry no root reaches, and return the counts."""
    reached = union_reached(graph, roots)
    rows = []
    for conversation_id, conversation in sorted(conversations.items()):
        for entry_id, entry in sorted(conversation["entries"].items()):
            if (conversation_id, entry_id) in reached:
                continue
            rows.append(
                {
                    "conversation": conversation_id,
                    "entry": entry_id,
                    "group": int(bool(entry.get("group"))),
                    "entry_title": (entry.get("title") or "").replace("\t", " "),
                    "conversation_title": (conversation["title"] or "").replace("\t", " "),
                }
            )

    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            delimiter="\t",
            lineterminator="\n",
            fieldnames=[
                "conversation",
                "entry",
                "group",
                "entry_title",
                "conversation_title",
            ],
        )
        writer.writeheader()
        writer.writerows(rows)

    return reached, rows


def describe(conversations, reached, rows, roots):
    """Print the counts, which are the headline."""
    entries = sum(len(c["entries"]) for c in conversations.values())
    groups = sum(1 for c in conversations.values() for e in c["entries"].values() if e.get("group"))
    unreachable_groups = sum(row["group"] for row in rows)
    dead = {row["conversation"] for row in rows}
    wholly_dead = {
        conversation_id
        for conversation_id in dead
        if not any((conversation_id, entry_id) in reached for entry_id in conversations[conversation_id]["entries"])
    }

    print()
    print(f"roots (entry 0 of a startable conversation)  {len(roots):>7}")
    print(f"conversations                                {len(conversations):>7}")
    print(f"entries                                      {entries:>7}")
    print(f"  of them group entries                      {groups:>7}")
    print(f"reached by some start                        {len(reached):>7}")
    print(f"REACHED BY NO START                          {len(rows):>7}")
    print(f"  of them group entries                      {unreachable_groups:>7}")
    print(f"  spread over conversations                  {len(dead):>7}")
    print(f"  conversations with NO reached entry        {len(wholly_dead):>7}")


def reachable_entries(index, trimmed, starts, out, roots_are_all, verify_only):
    conversations = read_index(index)
    graph = build_graph(conversations)

    print(f"{index}: {len(conversations)} conversations")
    verify(conversations, graph)
    if verify_only:
        return

    if trimmed is not None and trimmed.exists():
        shipped = read_index(trimmed)
        shipped_graph = build_graph(shipped)
        same = shipped_graph == graph
        print(f"  shipped index holds the same graph            {same}")
        if not same:
            raise SystemExit(
                f"{trimmed} walks differently from {index} - the mod's answers and these "
                "numbers are about different graphs"
            )

    if roots_are_all:
        startable = set(conversations)
        print("\nroots: entry 0 of EVERY conversation (--roots all)")
    else:
        startable = startable_conversations(starts)
        print(f"\nroots: entry 0 of the {len(startable)} startable conversations, per {starts}")

    roots = {
        (conversation_id, 0)
        for conversation_id in startable
        if 0 in conversations.get(conversation_id, {"entries": {}})["entries"]
    }
    missing = len(startable) - len(roots)
    if missing:
        print(f"  {missing} startable conversations have no entry 0 and cannot be a root")

    reached, rows = report(conversations, graph, roots, out)
    describe(conversations, reached, rows, roots)
    print(f"\nwritten to {out.resolve()}")


###############################################################################
# CLI
###############################################################################


def get_parser():
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--index", default=INDEX, type=Path, help="the conversation index")
    parser.add_argument(
        "--trimmed",
        default=TRIMMED,
        type=Path,
        help="the index as the mod ships it, walked as a cross-check",
    )
    parser.add_argument(
        "--starts",
        default=STARTS,
        type=Path,
        help="the dataset saying which conversations the game can start",
    )
    parser.add_argument("--out", default=OUT, type=Path, help="where to write the dataset")
    parser.add_argument(
        "--roots",
        choices=("startable", "all"),
        default="startable",
        help="whether a root is every conversation, or only one the game can start",
    )
    parser.add_argument(
        "--verify",
        action="store_true",
        help="only reproduce the matrix's candidate counts, and stop",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        reachable_entries(
            args.index,
            args.trimmed,
            args.starts,
            args.out,
            args.roots == "all",
            args.verify,
        )
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
