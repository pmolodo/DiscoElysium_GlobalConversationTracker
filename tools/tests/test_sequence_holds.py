"""What tools/sequence-holds.py calls a holding sequence, and what it leaves alone.

The point of the tool is a rule it does NOT overreach with: a sequence it has not been taught
about is reported as unknown rather than assumed either way. These pin the three verdicts and
the menu test they are applied to.
"""

import unittest

from drivers import load

holds = load("sequence-holds")


def entry(entry_id, actor="39", sequence=None, to=(), group=False):
    fields = {"Actor": actor}
    if sequence is not None:
        fields["Sequence"] = sequence
    return {"id": entry_id, "group": group, "to": list(to), "fields": fields}


class WhatHoldsTheLine(unittest.TestCase):
    """The verdict on a sequence, which is a named list rather than a guess."""

    def test_an_animation_holds(self):
        self.assertEqual(holds.verdict("PlayAnimation(Tequila Sunset, PBstart); HideActualHeld();"), "holds")

    def test_a_scheduled_command_holds_whatever_it_is(self):
        """An @ suffix gives a sequence a duration by construction, so the command need not be known."""
        self.assertEqual(holds.verdict("PostFX(blackout, true); LuaRun(x)@1.5; PostFX(blackout, false)@2;"), "holds")

    def test_the_orders_measured_to_pass_do_not_hold(self):
        self.assertEqual(holds.verdict('LuaRun(SetAreaState("GATES_MANANA","off"));'), "passes")
        self.assertEqual(holds.verdict("TravelTo(Kim Kitsuragi,KimGunsPos,0,true);"), "passes")

    def test_an_unteached_command_is_unknown_rather_than_assumed(self):
        """FocusCamera is the real one: 82 uses in the population and no measurement either way."""
        self.assertEqual(holds.verdict("FocusCamera(puke_cam);"), "unknown")


class WhatIsBehindAnEntry(unittest.TestCase):
    """Group entries are walked through, since the game expands them and displays nothing."""

    def test_a_menu_is_found_through_a_group(self):
        entries = {
            1: entry(1, to=[2]),
            2: entry(2, group=True, actor="0", to=[3, 4]),
            3: entry(3, actor=holds.OPTION_ACTOR),
            4: entry(4, actor=holds.OPTION_ACTOR),
        }
        self.assertEqual(holds.behind(entries, 1), "menu")

    def test_a_line_behind_is_not_a_menu(self):
        entries = {1: entry(1, to=[2]), 2: entry(2, actor="39")}
        self.assertEqual(holds.behind(entries, 1), "line")

    def test_nothing_behind_says_so(self):
        self.assertEqual(holds.behind({1: entry(1)}, 1), "nothing")


class TheCensus(unittest.TestCase):
    """Only entries with a menu behind them and a sequence beyond Continue() are counted."""

    def rows(self):
        return [
            {
                "id": 1467,
                "entries": [
                    entry(17, sequence="PlayAnimation(Tequila Sunset, PBstart);", to=[11]),
                    entry(11, group=True, actor="0", to=[148, 47]),
                    entry(148, actor=holds.OPTION_ACTOR),
                    entry(47, actor=holds.OPTION_ACTOR),
                    # A plain Continue() is the overwhelming majority and is not the question.
                    entry(43, sequence="Continue()", to=[17]),
                    # No menu behind it, so it is outside the population whatever it says.
                    entry(99, sequence="PlayAnimation(x, y);", to=[43]),
                ],
            }
        ]

    def test_only_the_holder_with_a_menu_behind_is_counted(self):
        counted, before_menu, _ = holds.census(self.rows())
        self.assertEqual([(c, e) for c, e, _ in before_menu["holds"]], [(1467, 17)])
        self.assertEqual(before_menu["passes"], [])

    def test_the_plain_sequences_are_counted_but_not_in_the_population(self):
        counted, before_menu, _ = holds.census(self.rows())
        self.assertEqual(counted["plain"], 1)
        self.assertEqual(counted["sequenced"], 3)


if __name__ == "__main__":
    unittest.main()
