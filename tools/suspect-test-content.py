#!/usr/bin/env python
# run-log-kind: analysis

"""What looks like test content, with each kind of evidence kept apart and weighed.

## THIS IS A HEURISTIC AND THE LISTING SAYS SO ON ITS FACE

Nothing here proves a conversation is test content. What prompted the tool is the mistake it
exists to stop: conversation 7 was called a "test dialogue" on the strength of its title
alone, stated as a fact. The honest position was weaker - the title says so, the entry text
corroborates it, and nothing had established that the game cannot start it.

So the evidence is reported in SEPARATE COLUMNS and never collapsed into one number that
hides which of them fired.

## Which signals are worth anything, MEASURED rather than assumed

The walk (`tools/reachable-entries.py`) knows nothing about vocabulary, so how often a
wording signal picks a conversation the walk finds wholly unreachable says what that signal
is worth. `--calibrate` prints that table, and every run prints it above the listing, so a
reader is never asked to take the ranking on trust. What it says, against a base rate of
about 14% of conversations being wholly unreached:

- A MARKER IN THE CONVERSATION TITLE fires 9 times and is right 9 times. All nine are
  unambiguous - "delete", "DELETE THIS FOLDER", "Coop TEST", "TEST / END TITLES TEST FOR
  ROZZO", "Stage directions test dialogue". This is the one wording signal worth having.
- A MARKER IN AN ENTRY TITLE fires 23 times and agrees twice: 9%, BELOW the base rate.
- A MARKER IN DIALOGUE TEXT fires 38 times and agrees three times: 8%, below it too. The
  matches are ordinary English - "Test the waters here", "Had a battery of tests last week",
  "some unused change in there". Conversation 7's "Test formatting for poem" and "Check
  tester" are real, and are the exception that made this look like a signal.

THAT INVERTS THE ORDERING ONE WOULD EXPECT. The guess going in was that dialogue text is
stronger than a title, because a writer is likelier to name a file badly than to put "Check
tester" in a shipped line. What actually separates the two is that a title is a NAME, chosen
once and deliberately, while dialogue text is prose - and prose in a 70,000-line script will
contain any common word many times, whatever it is about. So only the first of the three is
ranked on, and the other two are reported beside it as the circumstantial evidence they are.

SHAPE IS NOT DETECTABLE THIS WAY, and the column that tried is gone. A formatting testbed is
a grab-bag of lines from unrelated parts of the game, so counting distinct actors and check
types per conversation ought to find it. It does not: of 1,501 conversations ranked by
actors per entry, the four known test conversations come 1070th, 1233rd, 1291st and 1383rd -
the wrong end. What the metric actually measures is how big a conversation is, and the
listing was being reordered by conversation size wearing a theory's clothes. `actors` and
`skills` stay as columns to be read; nothing ranks on them.

## What this is not

NOT A CHANGE TO WHAT ANYTHING DOES. Nothing should start skipping content on the strength of
this listing without a separate decision; it is a thing to read, and every column in it is
circumstantial.
"""

import argparse
import csv
import json
import re
import sys
import traceback

from collections import defaultdict
from pathlib import Path

###############################################################################
# Core functions
###############################################################################

INDEX = Path(".game_reference_copies/derived/conversation_index.jsonl")
UNREACHED = Path("analysis/outputs/reachable-entries.tsv")
OUT = Path("analysis/outputs/suspect-test-content.tsv")

#: Words that are hard to explain away in something somebody NAMED. `delete` is here because
#: a writer writes it to mean it; `wip`, `todo` and `tbd` because they are notes to a
#: colleague. In prose all of them are just words, which is what the calibration shows.
STRONG = (
    "test",
    "tests",
    "tester",
    "testing",
    "debug",
    "placeholder",
    "dummy",
    "sandbox",
    "delete",
    "deleted",
    "wip",
    "todo",
    "tbd",
    "unused",
)

#: Words that are ALSO ordinary English even in a name. `old` describes half the people in
#: this game and `cut` is a verb. Reported so a reader can see them; never ranked on.
WEAK = ("old", "cut", "temp", "scratch", "junk")


def markers_in(text, words):
    """Which of `words` appear in `text` as whole words, lowercased."""
    if not text:
        return set()
    found = set()
    lowered = text.lower()
    for word in words:
        if re.search(rf"\b{re.escape(word)}\b", lowered):
            found.add(word)
    return found


def read_index(path):
    """Each conversation record, by id."""
    conversations = {}
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            record = json.loads(line)
            if "entries" in record:
                conversations[record["id"]] = record
    return conversations


def unreached_counts(path):
    """How many entries of each conversation no start reaches, or None if not run yet."""
    if not path.exists():
        return None
    counts = defaultdict(int)
    with path.open(encoding="utf-8", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            counts[int(row["conversation"])] += 1
    return counts


def evidence_for(conversation, unreached):
    """Every column, for one conversation.

    THE THREE WORDING SIGNALS ARE KEPT APART - a marker in the conversation's own title, in
    an entry's title, and in dialogue text - because they behave nothing like each other and
    a column that merged them would hide which one fired.
    """
    entries = conversation["entries"]
    title = conversation.get("title") or ""

    conv_title = markers_in(title, STRONG)
    entry_title = set()
    text = set()
    weak = markers_in(title, WEAK)
    actors = set()
    skills = set()

    for entry in entries:
        fields = entry.get("fields") or {}
        entry_title |= markers_in(entry.get("title") or "", STRONG)
        text |= markers_in(fields.get("Dialogue Text"), STRONG)
        weak |= markers_in(entry.get("title") or "", WEAK)
        if fields.get("Actor"):
            actors.add(fields["Actor"])
        if fields.get("SkillType"):
            skills.add(fields["SkillType"])

    dead = unreached.get(conversation["id"], 0) if unreached is not None else 0
    return {
        "conversation": conversation["id"],
        "entries": len(entries),
        # THE RANK, and only these two earn it: a name somebody chose, and a walk that never
        # read a word. See the module doc for what the other columns measured.
        "ranked_on": int(bool(conv_title)) + int(bool(dead)),
        "conv_title_marks": ",".join(sorted(conv_title)),
        "unreached": dead,
        "wholly_unreached": int(bool(entries) and dead == len(entries)),
        "entry_title_marks": ",".join(sorted(entry_title)),
        "text_marks": ",".join(sorted(text)),
        "weak_marks": ",".join(sorted(weak)),
        "actors": len(actors),
        "skills": len(skills),
        "title": title.replace("\t", " "),
    }


def calibrate(rows, unreached):
    """What each wording signal is worth, against the one that ignores wording."""
    if unreached is None:
        print("\nCALIBRATION NEEDS THE WALK - run tools/reachable-entries.py first.")
        return

    wholly = {row["conversation"] for row in rows if row["wholly_unreached"]}
    base = len(wholly) / len(rows)
    print()
    print("WHAT EACH SIGNAL IS WORTH, against the walk, which never reads a word")
    print(f"  {'signal':<20} {'fires':>6} {'agrees':>7} {'precision':>10}")
    for label, column in (
        ("conversation title", "conv_title_marks"),
        ("entry title", "entry_title_marks"),
        ("dialogue text", "text_marks"),
    ):
        fired = [row for row in rows if row[column]]
        agreed = sum(1 for row in fired if row["wholly_unreached"])
        share = agreed / len(fired) if fired else 0.0
        verdict = "worth ranking on" if share > base * 2 else "at or below chance"
        print(f"  {label:<20} {len(fired):>6} {agreed:>7} {share:>9.1%}  {verdict}")
    print(f"  {'(base rate)':<20} {len(rows):>6} {len(wholly):>7} {base:>9.1%}")


def believability(listed, rows):
    """The paragraph the listing must be read with, with this run's numbers in it."""
    both = [row for row in listed if row["ranked_on"] == 2]
    named_only = [row for row in listed if row["conv_title_marks"] and not row["unreached"]]
    dead_only = [
        row for row in listed if row["wholly_unreached"] and not row["conv_title_marks"] and not row["text_marks"]
    ]

    print()
    print("HOW FAR TO BELIEVE THESE ROWS")
    print()
    print(
        f"  {len(both)} conversations are BOTH named like test content and reached by no\n"
        "  start. Those are as close to settled as anything here gets, because the two\n"
        "  signals cannot fail together: one is a word somebody typed, the other is a walk\n"
        "  over links that never reads a word."
    )
    print()
    print(
        f"  {len(named_only)} are NAMED like test content and the walk reaches them anyway.\n"
        "  A joke title on live content looks exactly like this, so read the content before\n"
        "  believing the name."
    )
    print()
    print(
        f"  {len(dead_only)} have NO entry any start reaches and nothing in their wording to\n"
        "  say why. The most interesting rows and the least explained: the evidence does not\n"
        "  depend on anybody's vocabulary, but neither does it distinguish test content from\n"
        "  content that was simply cut loose and left in the database."
    )
    print()
    print(
        "  NONE OF THIS PROVES ANYTHING. The walk ignores guards, so its reachable side is a\n"
        "  superset of what a player can see; and a conversation nothing starts today may be\n"
        "  started by a mechanism no exported table models. Do not skip content on the\n"
        "  strength of a row here without deciding to, separately and on purpose."
    )


def suspect(index, unreached_path, out, top, calibrate_only):
    conversations = read_index(index)
    unreached = unreached_counts(unreached_path)
    print(f"{index}: {len(conversations)} conversations")
    if unreached is None:
        print(f"{unreached_path}: absent - the walk column will read 0 for every row")

    rows = [evidence_for(conversation, unreached) for conversation in conversations.values()]
    calibrate(rows, unreached)
    if calibrate_only:
        return

    listed = [row for row in rows if row["ranked_on"]]
    listed.sort(
        key=lambda row: (
            -row["ranked_on"],
            -row["wholly_unreached"],
            -row["unreached"],
            row["conversation"],
        )
    )

    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            delimiter="\t",
            lineterminator="\n",
            fieldnames=list(rows[0]),
        )
        writer.writeheader()
        writer.writerows(listed)

    named = sum(1 for row in listed if row["conv_title_marks"])
    print(f"\nlisted                                   {len(listed)}")
    print(f"  named AND unreached                    {sum(1 for r in listed if r['ranked_on'] == 2)}")
    print(f"  named like test content                {named}")
    print(f"  reached by no start, at all            {sum(1 for r in listed if r['wholly_unreached'])}")
    print(
        "  reached by no start, in part          "
        f" {sum(1 for r in listed if r['unreached'] and not r['wholly_unreached'])}"
    )

    print(f"\nthe first {top}:")
    print(f"  {'conv':>5} {'entr':>5} {'dead':>5}  {'title':<46} named as")
    for row in listed[:top]:
        print(
            f"  {row['conversation']:>5} {row['entries']:>5} {row['unreached']:>5}  "
            f"{row['title'][:46]:<46} {row['conv_title_marks'] or '-'}"
        )

    believability(listed, rows)
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
        "--unreached",
        default=UNREACHED,
        type=Path,
        help="what tools/reachable-entries.py wrote",
    )
    parser.add_argument("--out", default=OUT, type=Path, help="where to write the listing")
    parser.add_argument("--top", default=30, type=int, help="how many rows to print")
    parser.add_argument(
        "--calibrate",
        action="store_true",
        help="only print what each wording signal is worth, and stop",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    parser = get_parser()
    args = parser.parse_args(argv)
    try:
        suspect(args.index, args.unreached, args.out, args.top, args.calibrate)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
