"""What tools/arm-oracle.py decides about two measured arms.

The bound it computes is what gates a day's work on a heuristic, so it is worth being sure it
counts the right groups and cannot be read as better than it is.
"""

import unittest

from pathlib import Path
from tempfile import TemporaryDirectory

from drivers import load

oracle = load("arm-oracle")


TAB = "\t"
COMBINED = ["conv", "runs", "menu_ms_median", "menu_ms_min", "menu_ms_max", "nodes_median", "rounds", "settled"]
PER_RUN = ["conv", "entries", "options", "offered", "menu_ms", "setup_ms", "asked", "rounds", "settled"]


def write(path, header, rows):
    lines = [TAB.join(header)]
    for row in rows:
        lines.append(TAB.join(str(row.get(column, 0)) for column in header))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="")


def arm(root, name, groups):
    """One measured folder, from {conversation: (menu_ms, nodes, asked)}."""
    folder = root / name
    write(
        folder / "combined.tsv",
        COMBINED,
        [
            {"conv": conv, "runs": 1, "menu_ms_median": ms, "nodes_median": nodes}
            for conv, (ms, nodes, _) in groups.items()
        ],
    )
    write(
        folder / "run-1" / "menus.tsv",
        PER_RUN,
        [{"conv": conv, "menu_ms": ms, "asked": asked} for conv, (ms, _, asked) in groups.items()],
    )
    return folder


class WhatIsCounted(unittest.TestCase):
    """Only the groups where the arms did different work, and the bound over those."""

    def pair(self, first, second):
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            return oracle.moved_groups(arm(root, "a", first), arm(root, "b", second), by="asked")

    def test_a_group_whose_nodes_did_not_move_is_left_out(self):
        """Identical node counts mean identical work, so the milliseconds are the machine.

        Counting them would dilute the bound with noise, and in the direction that makes a
        heuristic look more attractive than it is: every such group contributes whichever run
        happened to be faster.
        """
        rows = self.pair(
            {1: (100, 5000, 3), 2: (50, 700, 2)},
            {1: (90, 5000, 3), 2: (40, 900, 2)},
        )
        self.assertEqual([row["conv"] for row in rows], [2])

    def test_the_bound_is_the_better_arm_of_each_group(self):
        rows = self.pair(
            {1: (100, 5000, 3), 2: (50, 700, 9)},
            {1: (90, 4000, 3), 2: (80, 900, 9)},
        )
        self.assertEqual(sum(min(row["a"], row["b"]) for row in rows), 140)
        self.assertEqual(sum(row["a"] for row in rows), 150)
        self.assertEqual(sum(row["b"] for row in rows), 170)

    def test_the_feature_comes_off_the_runs(self):
        """`asked` is written per run and never reaches combined.tsv, so a sweep needs it folded."""
        rows = self.pair({1: (100, 5000, 7)}, {1: (90, 4000, 7)})
        self.assertEqual(rows[0]["feature"], 7)

    def test_arms_that_did_the_same_work_everywhere_are_refused(self):
        """Rather than reporting a bound of zero, which reads like a measured answer."""
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            same = {1: (100, 5000, 3)}
            with self.assertRaises(SystemExit):
                oracle.report(arm(root, "a", same), arm(root, "b", same))


if __name__ == "__main__":
    unittest.main()
