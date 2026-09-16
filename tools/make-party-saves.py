#!/usr/bin/env python

"""Write the party saves: one per combination of the flags IsKimHere might read.

MADE RATHER THAN PLAYED, for the reason the weather saves are. Reaching each of these
states in game means sending Kim away, sleeping, waiting for morning - and every one of
those moves the clock, the thoughts cooking and whatever else is on a timer, so two saves
meant to differ in the party would differ in a dozen things. A hand-made diff differs in
the party and nothing else.

## WHAT THE QUESTION IS

`PartyManager.IsKimHere` has no readable body in Final Cut: that build is IL2CPP, so the
export stubs it to `return false`. The pre-final-cut build is Mono and its body IS readable:

    if (SingletonComponent<KimKitsuragi>.Singleton.IsInParty)
        return !SingletonComponent<KimKitsuragi>.Singleton.IsLeftOutside;
    return false;

Two flags, and no others. But the save carries more that read as though they belonged -
`isKimAwayUpToMorning`, which exists in both builds and which that body demonstrably does
not read, and `isKimSleepingInHisRoom`, which is FINAL CUT ONLY: it appears in Final Cut's
`global-metadata.dat` and nowhere in the entire pre-final-cut install. So for that one the
build that has the flag is the build whose body is stripped, and no amount of reading
settles it.

These saves settle it by measurement instead. Load each in Final Cut, evaluate
`IsKimHere()`, and the truth table says which flags it reads.

## TWENTY-FOUR COMBINATIONS, BECAUSE THE OTHER EIGHT WERE MEASURED AND ARE INVALID

All five flags vary independently, which would be thirty-two saves. Eight are skipped: the
ones with `isKimInParty` false AND `isKimAbandoned` false. `PartyPersister.Deserialize`
reads:

    if (partyState.isKimInParty)        PartyManager.ReturnKitsuragiToParty();
    else if (partyState.isKimAbandoned) PartyManager.RemoveKitsuragiFromParty();
    else                                Debug.LogError("Kim should either be in party or abandoned");

    RestoreReturnState(partyState.isKimAwayUpToMorning);
    Kim.IsLeftOutside = partyState.isKimLeftOutside;   // unconditional

With both false it takes the `else`, which only logs - so party membership is never assigned
and keeps whatever the previously loaded save left, while `IsLeftOutside` is assigned anyway.
A PARTIAL load, and the game itself calls it invalid: that `LogError` is the game saying so.

THIS WAS NOT ASSUMED, IT WAS MEASURED, and the distinction is the whole history of this file.
The first version of this script derived `isKimAbandoned` and excluded those rows on exactly
the reasoning above - which came from the pre-final-cut body, the one source already
established as unable to speak for Final Cut. That exclusion was removed, all thirty-two were
run, and the shipped build then showed the eight failing in two distinct ways that one
mechanism explains, with no other save failing. Only now that it is a finding rather than a
guess is it encoded here.

WHY SKIPPING THEM MATTERS beyond tidiness: loading an invalid save leaves the game in a state
nothing intended, and the run that measured this had those loads INTERLEAVED with the good
ones. Every good row was control-verified, so the table stands - but a run that never enters
the invalid state cannot be contaminated by it at all. Pass `--include-invalid` to write all
thirty-two, which is what re-measuring the constraint would need.

## WHICH MAKES LOAD ORDER A VARIABLE

If the game really does call neither restore for some row, then `IsKimHere()` there answers
about whatever was loaded BEFORE it, not about the row. That is a measurable claim rather
than a blank - but it means the rows are not independent, so a run records which save each
row followed and does not treat a carried-over answer as a property of the save. Two passes
in different orders tell a carried-over row from a real one.

## AND THE LOAD PATH IS NOT A PLAIN RESTORE

The same pre-final-cut method continues:

    PartyManager.RestoreReturnState(partyState.isKimAwayUpToMorning);
    SingletonComponent<KimKitsuragi>.Singleton.IsLeftOutside = partyState.isKimLeftOutside;

`ReturnKitsuragiToParty` sets `IsLeftOutside` false and `IsInParty` true, and only the line
after it assigns the saved `IsLeftOutside`. The order saves us there - the flags do
round-trip - but it is exactly the shape that caught the weather saves, where setting the
variable without the preset was silently overwritten on load. So a run reads the loaded
state back rather than assuming it survived.

## AND A GUESS, WHICH IS A SCHEDULE RATHER THAN AN ANSWER

The run loads a control save before each test save, so that a load which did not take can
be told from one that did: a stale answer is the CONTROL'S answer, so a test save whose
answer differs from it demonstrably loaded. A row that diverges is settled and stops there,
which means the control worth trying FIRST is the one that disagrees with what the save is
expected to say.

So this also writes a prediction: per save, the control to try first, chosen as the opposite
of what `predicted` below says that save answers. Guessing right settles a row in one control
instead of two; guessing wrong costs the second control and changes no answer, because what
settles a row is a divergence the run observed rather than anything predicted here. That is
the only reason a guess is allowed to touch this at all.

## WHERE THEY GO

Into a build directory, not `testing/scenarios`. These are measurement fixtures rather than
committed scenarios, and `testing/` itself is out of the question: `TemplateSaveTests`
refuses to find more than one expanded save directly under it.
"""

import argparse
import itertools
import json
import os
import pathlib
import shutil
import sys
import traceback

###############################################################################
# Core functions
###############################################################################

EXPANDED_SUFFIX = ".ntwtf"

TEMPLATE = "save_template"

# The member the party lives in, which is the one these change rather than inherit.
PARTY_SUFFIX = ".1st.ntwtf.json"

# The members every save carries. All but the first are inherited whole.
SUFFIXES = [PARTY_SUFFIX, ".2nd.ntwtf.json", ".FOW.json", ".states.lua"]

# Every flag that varies, in the order a save's name spells them. ALL OF THEM VARY
# INDEPENDENTLY - see the module docstring on why nothing is excluded.
VARYING = [
    "isKimInParty",
    "isKimLeftOutside",
    "isKimAbandoned",
    "isKimAwayUpToMorning",
    "isKimSleepingInHisRoom",
]

NAME_PREFIX = "party"

# Where the index of what was written goes, so a run can join answers to inputs.
INDEX_NAME = "party-saves.json"

# Where the predicted first control per save goes.
FIRST_CONTROL_NAME = "party-first-control.json"

# The two saves a run uses as controls, and what each is expected to answer. They are
# ordinary members of the set: one has Kim in the party and not left outside, the other
# has him neither in the party nor anywhere else, which are the two combinations the
# readable body and the load path both agree on.
TRUE_CONTROL = "party-10000"
FALSE_CONTROL = "party-00100"


def predicted(flags):
    """What IsKimHere is guessed to answer: the pre-final-cut body, and nothing else.

    Two flags, because that is what the one readable body names. It is a GUESS about the
    Final Cut build, whose body is stripped - and it is used only to order the controls,
    so being wrong costs a second control and changes no answer.
    """
    return flags["isKimInParty"] and not flags["isKimLeftOutside"]


def name_for(flags):
    """A save's name, spelling its flags in VARYING's order as ones and zeros."""
    digits = "".join("1" if flags[flag] else "0" for flag in VARYING)
    return f"{NAME_PREFIX}-{digits}"


def member_for(name, suffix):
    """One manifest entry: a diff of its own where it has one, inherited where not."""
    if suffix != PARTY_SUFFIX:
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


def manifest_for(name, template_relative):
    return {
        "_format": "expanded-save-diff",
        "_formatVersion": 1,
        "base": template_relative,
        "members": [member_for(name, suffix) for suffix in SUFFIXES],
    }


def party_diff(template_relative, flags):
    """The party state, as a diff of the template's own first blob."""
    return {
        "_base": f"{template_relative}/{TEMPLATE}{PARTY_SUFFIX}",
        "_changes": {"partyState": dict(flags)},
        "_format": "json-diff",
        "_formatVersion": 1,
    }


def write_json(path, document):
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(document, handle, indent=2, ensure_ascii=False, sort_keys=True)
        handle.write("\n")


def valid(flags):
    """Whether the game's own loader has a branch for this combination.

    MEASURED, not assumed - see the module docstring. Deserialize restores party membership
    only when isKimInParty is true or isKimAbandoned is true; with both false it logs
    "Kim should either be in party or abandoned" and assigns neither, so the save loads only
    partly. Confirmed on the shipped build by de-h0f1.33, where all eight such saves failed
    their checks and no other save did.
    """
    return flags["isKimInParty"] or flags["isKimAbandoned"]


def combinations(include_invalid=False):
    """Every combination of the varying flags, as dictionaries."""
    for values in itertools.product([False, True], repeat=len(VARYING)):
        flags = dict(zip(VARYING, values))
        if include_invalid or valid(flags):
            yield flags


def make(repo, out_dir, include_invalid=False):
    repo = pathlib.Path(repo).resolve()
    template = repo / "testing" / f"{TEMPLATE}{EXPANDED_SUFFIX}"
    if not template.is_dir():
        raise RuntimeError(f"{template} is not there, and it is what these diff against")

    out = pathlib.Path(out_dir)
    if not out.is_absolute():
        out = repo / out
    out.mkdir(parents=True, exist_ok=True)

    # A STALE SAVE IS A TRAP, not clutter. A run loads every expanded save it finds in the
    # folder, so one left behind by an earlier generation would be loaded as though it had
    # been asked for - and the whole point of skipping the invalid pair is that the game
    # never enters it. So the folder is made to match exactly what was asked for.
    wanted = {name_for(flags) for flags in combinations(include_invalid)}
    for existing in sorted(out.glob(f"{NAME_PREFIX}-*{EXPANDED_SUFFIX}")):
        if existing.is_dir() and existing.name[: -len(EXPANDED_SUFFIX)] not in wanted:
            shutil.rmtree(existing)
            print(f"  removed stale {existing.name}")

    written = []
    first_control = {}
    for flags in combinations(include_invalid):
        name = name_for(flags)
        save = out / f"{name}{EXPANDED_SUFFIX}"

        # Regenerated rather than refused: this writes into a build directory, so a
        # re-run is the ordinary case rather than a mistake to report.
        if save.exists():
            shutil.rmtree(save)

        # Relative to the SAVE's own directory, which is what the manifest and the diff
        # are both resolved against. Computed rather than spelled, because the output
        # directory is an argument and a hard-coded number of dots would be wrong the
        # first time somebody moved it.
        template_relative = os.path.relpath(template, save).replace(os.sep, "/")

        write_json(save / "_archive.json", manifest_for(name, template_relative))
        write_json(save / f"{name}{PARTY_SUFFIX}", party_diff(template_relative, flags))

        # THE OPPOSITE OF THE GUESS, because the control that disagrees is the one that
        # diverges at once and settles the row without a second.
        expected = bool(predicted(flags))
        if name not in (TRUE_CONTROL, FALSE_CONTROL):
            first_control[name] = FALSE_CONTROL if expected else TRUE_CONTROL

        written.append({"expected": expected, "flags": dict(flags), "save": name})
        spelled = ", ".join(f"{flag}={flags[flag]}" for flag in VARYING)
        print(f"  {name}: {spelled}")

    # The index, so a run joins an answer to the flags that produced it rather than
    # re-deriving them from the name. The expectation rides along as a RECORD of what was
    # guessed, which is not the same as something the run is held to.
    write_json(out / INDEX_NAME, {"saves": written, "varying": VARYING})
    write_json(out / FIRST_CONTROL_NAME, first_control)

    expected_true = sum(1 for save in written if save["expected"])
    print(f"{len(written)} save(s) written to {out}")
    print(f"and {INDEX_NAME} beside them, naming the flags each one carries")
    print(
        f"{FIRST_CONTROL_NAME} guesses {expected_true} of {len(written)} answer true, "
        f"so those try {FALSE_CONTROL} first and the rest try {TRUE_CONTROL}"
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
        "--out-dir",
        default=".build/party-saves",
        help="where the saves go, relative to the repository root unless absolute",
    )
    parser.add_argument(
        "--include-invalid",
        action="store_true",
        help=(
            "also write the eight combinations the game's loader has no branch for, "
            "which load only partly and leave it in a state nothing intended"
        ),
    )
    return parser


def main(argv=None):
    if argv is None:
        argv = sys.argv[1:]
    args = get_parser().parse_args(argv)
    try:
        make(args.repo, args.out_dir, args.include_invalid)
    except Exception:  # pylint: disable=broad-except
        traceback.print_exc()
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
