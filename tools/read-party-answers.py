#!/usr/bin/env python

"""Read the party truth table back, and say which flags IsKimHere actually reads.

WHAT THIS IS FOR. `PartyManager.IsKimHere` has no readable body in Final Cut - that build
is IL2CPP and the export stubs it to `return false`. The pre-final-cut body is readable and
names two flags:

    if (SingletonComponent<KimKitsuragi>.Singleton.IsInParty)
        return !SingletonComponent<KimKitsuragi>.Singleton.IsLeftOutside;
    return false;

The save carries three more that read as though they might belong. `tools/make-party-saves.py`
writes all 32 combinations, the harness's evaluate verb asks the running game what
`IsKimHere()` answers in each, and this reads the answers back as a function of the flags.

## WHAT IT REFUSES TO DO

IT DOES NOT TRUST A ROW THAT WAS NOT CONTROLLED. A save whose state the game's loader will
not take answers about whatever was loaded BEFORE it, and such a row looks exactly like a
real one. The run loads a control first for that reason: a test answer that DIFFERS from the
control's proves the state changed. A row that never diverged from any control did not load,
and is reported as unusable rather than folded into the table.

IT ALSO CHECKS THE LOAD INDEPENDENTLY, where `IsKimInParty()` was asked. That one is a raw
flag end to end - the Lua function returns `KimKitsuragi.IsInParty`, which the save records
as `partyState.isKimInParty` - so an answer disagreeing with the save's own flag means the
save's state did not fully arrive, whatever the controls said.

AND THAT IS A DIFFERENT FAILURE FROM THE ONE ABOVE, which is why both checks exist.
Measured on the shipped build: `Deserialize` assigns `IsLeftOutside` unconditionally, but
only touches `IsInParty` when one of its two branches is taken - so a save carrying neither
ends up mixing its own left-outside flag with the PREVIOUS save's party membership. Such a
row diverges from its control quite happily, because the state really did change; it just
changed into something no save ever described. The control cannot catch that and this can.

## HOW IT DECIDES WHICH FLAGS MATTER

A flag is IRRELEVANT if flipping it, with every other flag held fixed, never changes the
answer. That is read off the measured pairs rather than fitted: with every combination
present each flag has sixteen such pairs, and one disagreement is enough to make it relevant.

AND A FLAG WITH NO PAIRS IS NOT IRRELEVANT, IT IS UNTESTED. The two are reported as
different things on purpose. A row can be missing because the game never answered it or
because no control proved it loaded, and a run that lost half its rows would otherwise
report the flags it never examined as ones the function demonstrably ignores - which is
absence of evidence dressed up as evidence of absence, and is the exact mistake the whole
party investigation exists to correct.
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

HERE = "IsKimHere()"
IN_PARTY = "IsKimInParty()"

# The flag IsKimInParty is a direct read of, which is what makes it a load check.
IN_PARTY_FLAG = "isKimInParty"

CONTROL_ROLE = "control"
TEST_ROLE = "test"


def load(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def value_of(row):
    """One answer as a comparable token, or None where the game did not answer."""
    if not row.get("read"):
        return None
    return row.get("value")


def controlled(rows):
    """Test rows paired with the control that preceded them, in run order.

    Yields (control_row, test_row) for rows of the same expression, which is the only
    shape a divergence can be read from.
    """
    last_control = {}
    for row in rows:
        key = row["expression"]
        if row["role"] == CONTROL_ROLE:
            last_control[key] = row
        elif row["role"] == TEST_ROLE:
            yield last_control.get(key), row


def resolved_answers(rows):
    """What each save answered, keeping only rows a control proved had loaded.

    Returns (answers, unusable), where answers maps save to expression to value.
    """
    diverged = set()
    seen = {}
    for control, test in controlled(rows):
        save = test["save"]
        seen.setdefault(save, {})[test["expression"]] = value_of(test)
        if control is not None and value_of(control) != value_of(test):
            diverged.add(save)

    answers = {save: got for save, got in seen.items() if save in diverged}
    unusable = sorted(set(seen) - diverged)
    return answers, unusable


def load_disagreements(answers, flags_of):
    """Saves whose IsKimInParty answer contradicts the flag the save carries.

    Returns (wrong, checked). The count of what was CHECKED rides along because an empty
    check is not a passed one, and reporting it as though it were is how a run that
    measured nothing comes to look like a run that measured everything.
    """
    wrong = []
    checked = 0
    for save, got in sorted(answers.items()):
        if IN_PARTY not in got or got[IN_PARTY] is None:
            continue
        checked += 1
        if bool(got[IN_PARTY]) != bool(flags_of[save][IN_PARTY_FLAG]):
            wrong.append(save)
    return wrong, checked


def shared_flags(saves, flags_of):
    """The flag settings every one of these saves has in common.

    WHY THIS IS WORTH COMPUTING. A save whose state never took is not simply a lost row.
    If every such save shares a flag setting, that setting is a LEAD about what the loader
    does with it - a finding about the game rather than a gap in the run.

    A LEAD AND NOT A VERDICT, though. A run with a fixed processing order groups saves by
    name, so a setting shared by every failure may be shared only because those saves ran
    together; position is confounded with it, and only re-running in another order tells
    the two apart.
    """
    common = None
    for save in saves:
        current = {flag: bool(value) for flag, value in flags_of[save].items()}
        if common is None:
            common = current
            continue
        common = {flag: value for flag, value in common.items() if flag in current and current[flag] == value}

    return common or {}


def flag_evidence(answers, flags_of, varying):
    """What the measured rows say about each flag, and the table they say it from.

    Returns (evidence, by_key), where evidence maps a flag to (tested, disagreed) counted
    in PAIRS. Counting rather than returning a list of relevant flags is the point: a flag
    with no pairs was not shown to be ignored, it was never examined, and only a count can
    tell those apart.
    """
    by_key = {}
    for save, got in answers.items():
        if HERE not in got or got[HERE] is None:
            continue
        flags = flags_of[save]
        by_key[tuple(bool(flags[flag]) for flag in varying)] = bool(got[HERE])

    evidence = {}
    for index, flag in enumerate(varying):
        tested = 0
        disagreed = 0
        for key, answer in by_key.items():
            flipped = list(key)
            flipped[index] = not flipped[index]
            other = by_key.get(tuple(flipped))
            if other is None:
                continue
            tested += 1
            if other != answer:
                disagreed += 1

        # Halved because each unordered pair is met from both of its ends.
        evidence[flag] = (tested // 2, disagreed // 2)

    return evidence, by_key


def report(answers_path, index_path):
    answers_file = load(answers_path)
    index = load(index_path)

    varying = index["varying"]
    flags_of = {save["save"]: save["flags"] for save in index["saves"]}

    answers, unusable = resolved_answers(answers_file["answers"])

    print(f"asked:     {', '.join(answers_file.get('asked', []))}")
    print(f"controls:  {', '.join(answers_file.get('controls', [])) or 'none'}")
    print(f"usable:    {len(answers)} of {len(flags_of)} saves")
    if unusable:
        # "Their state never took" rather than "they never loaded": the save itself loads,
        # and what may not happen is the restore of the part being measured.
        print(f"UNUSABLE:  {len(unusable)} save(s) never diverged from any control, so their state never took:")
        for save in unusable:
            print(f"             {save}")

        shared = shared_flags(unusable, flags_of)
        if shared:
            spelled = ", ".join(f"{flag}={value}" for flag, value in sorted(shared.items()))
            print(f"             every one of them carries {spelled}")
            # A LEAD, NOT A VERDICT. A run with a fixed processing order groups saves by
            # name, so a flag shared by every failure may be shared only because those
            # saves ran first - position and flag are confounded, and only re-running in
            # another order can separate them.
            print(
                "             - a lead, not a verdict: if they also ran consecutively, "
                "position is confounded with that setting"
            )

    wrong, checked = load_disagreements(answers, flags_of)
    if checked == 0:
        print(f"load check: nothing to check - no usable save answered {IN_PARTY}")
    elif wrong:
        print(
            f"CONTRADICTED: {len(wrong)} of {checked} save(s) answered {IN_PARTY} against "
            f"their own {IN_PARTY_FLAG}, so their state only PARTLY restored:"
        )
        for save in wrong:
            print(f"             {save}")

        # THE LOAD HAPPENED. What these rows show is a PARTIAL restore: measured on the
        # shipped build, Deserialize assigns IsLeftOutside unconditionally and only touches
        # IsInParty when one of its two branches is taken - so a save with neither can end
        # up mixing its own left-outside flag with the previous save's party membership.
        # Such a row DIVERGES from its control while describing a state no save asked for,
        # which is why the control check cannot catch it and this one can.
        shared = shared_flags(wrong, flags_of)
        if shared:
            spelled = ", ".join(f"{flag}={value}" for flag, value in sorted(shared.items()))
            print(f"             every one of them carries {spelled}")
    else:
        print(f"load check: all {checked} save(s) answered {IN_PARTY} as their own flag")

    # EXCLUDED FROM THE TABLE, not merely reported. A row that contradicts its own flag was
    # only PARTLY restored, so its answer describes a state no save ever asked for - and it
    # can still DIVERGE from its control, which is why the control check alone cannot catch
    # it. Measured: party-01000 answered IsKimInParty true while carrying isKimInParty
    # false, because Deserialize restores IsLeftOutside unconditionally and IsInParty only
    # when one of its two branches is taken. Believing that row would put a combination in
    # the table that the game was never in.
    trusted = {save: got for save, got in answers.items() if save not in set(wrong)}

    evidence, by_key = flag_evidence(trusted, flags_of, varying)
    reads = [flag for flag in varying if evidence[flag][1]]
    print()
    print(f"{HERE} depends on: {', '.join(reads) or 'nothing measurable'}")
    for flag in varying:
        tested, disagreed = evidence[flag]
        if disagreed:
            continue
        if tested == 0:
            print(f"  UNTESTED  {flag}: no measured pair differed in it alone")
        else:
            print(f"  NOT read  {flag}: {tested} pair(s) differed in it alone, all agreeing")

    print()
    print(f"the table, as measured ({len(by_key)} of 32 combinations):")
    header = "  " + "  ".join(f"{flag:>22}" for flag in varying) + "   IsKimHere"
    print(header)
    for key in sorted(by_key):
        cells = "  ".join(f"{str(bool(part)):>22}" for part in key)
        print(f"  {cells}   {by_key[key]}")

    # The one readable body, checked against rather than assumed.
    print()
    disagreed = [
        key
        for key, answer in by_key.items()
        if answer != (key[varying.index("isKimInParty")] and not key[varying.index("isKimLeftOutside")])
    ]
    if not by_key:
        print("nothing usable to compare against the pre-final-cut body")
    elif disagreed:
        print(f"DISAGREES with the pre-final-cut body in {len(disagreed)} of {len(by_key)} combinations")
        for key in sorted(disagreed):
            spelled = ", ".join(f"{flag}={bool(part)}" for flag, part in zip(varying, key))
            print(f"  {spelled} -> {by_key[key]}")
    else:
        print(
            f"AGREES with the pre-final-cut body in all {len(by_key)} measured "
            "combinations: isKimInParty and not isKimLeftOutside"
        )


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
        "--answers",
        default=".build/automation/evaluate-answers.json",
        help="what the evaluate verb wrote",
    )
    parser.add_argument(
        "--index",
        default=".build/party-saves/party-saves.json",
        help="what make-party-saves.py wrote, naming each save's flags",
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    repo = pathlib.Path(args.repo).resolve()
    try:
        report(repo / args.answers, repo / args.index)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
