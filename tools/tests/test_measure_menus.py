"""What tools/measure-menus.py decides, asked of it directly.

WHAT IS WORTH A TEST HERE is the logic the driver carries that no measurement would catch: the
guard that refuses a pass whose binary moved under it, and the folding of several runs into one
table. Both are decisions rather than measurements - they are right or wrong on their inputs,
with no machine and no game in the way - so they can be asked in milliseconds.

WHAT IS NOT TESTED HERE is anything that needs a built binary or a running game. Those are the
suites' job, and a Python test that shelled out to a real pass would be a slow, flaky copy of
one.
"""

import unittest

from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path
from tempfile import TemporaryDirectory

from drivers import load

menus = load("measure-menus")


TAB = "\t"
HEADER = ["conv", "entries", "options", "offered", "menu_ms", "nodes", "rounds", "settled", "starred"]


def write_run(folder, rows):
    """One run's menus.tsv, from {conversation: {column: value}}, under the real header."""
    folder.mkdir(parents=True, exist_ok=True)
    lines = [TAB.join(HEADER)]
    for conversation, row in sorted(rows.items()):
        full = {
            "conv": conversation,
            "entries": 1,
            "options": 8,
            "offered": 8,
            "rounds": 1,
            "settled": 8,
            "starred": "",
        }
        full.update(row)
        lines.append(TAB.join(str(full[column]) for column in HEADER))
    (folder / "menus.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="")


def summary_of(runs):
    """The summary text `combine` writes for these runs, each a {conversation: {column: value}}.

    `combine` prints the summary as well as writing it, which is what a person running a
    measurement wants and not what a test run wants, so the print is swallowed and the written
    file is what is read. Reading the file is also the stronger check: it is what every later
    tool opens.
    """
    with TemporaryDirectory() as temporary:
        root = Path(temporary)
        folders = []
        for number, rows in enumerate(runs, start=1):
            folder = root / f"run-{number}"
            write_run(folder, rows)
            folders.append(folder)
        out = root / "out"
        out.mkdir()
        with redirect_stdout(StringIO()):
            menus.combine(folders, out)
        return (out / menus.SUMMARY).read_text(encoding="utf-8")


class BinaryDigestGuard(unittest.TestCase):
    """A pass measures one binary, and says so rather than measuring two.

    The guard never executes what it is given, only hashes it, so an ordinary file stands in for
    a built measurement. Provoking it through a real pass would mean rewriting a running
    executable inside a window of a few seconds, which is both unreliable on Windows and green
    for the wrong reason when the window is missed.
    """

    def setUp(self):
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "menu_matrix.exe"
        self.binary.write_bytes(b"a build")
        self.built_at = menus.common.binary_digest(self.binary)

    def run_over(self, digest):
        return menus.Run(self.root / "rows", str(self.binary), digest)

    def test_unchanged_binary_is_accepted(self):
        run = self.run_over(self.built_at)
        self.assertTrue(run.folder.exists())
        self.assertEqual(run.rows, run.folder / "menus.tsv")

    def test_changed_binary_is_refused(self):
        """And says which two builds it is between, since that is what makes it actionable.

        The refusal goes to stderr, which is read here rather than let through: a passing run
        that prints "Something rebuilt it between runs" is a passing run somebody stops to
        read.
        """
        self.binary.write_bytes(b"a different build")
        said = StringIO()
        with redirect_stderr(said), self.assertRaises(SystemExit) as refusal:
            self.run_over(self.built_at)
        self.assertEqual(refusal.exception.code, 2)
        self.assertIn(self.built_at[:12], said.getvalue())
        self.assertIn(menus.common.binary_digest(self.binary)[:12], said.getvalue())


class NodesDrift(unittest.TestCase):
    """The summary says when a nodes reading moved between runs, and when it did not.

    de-jitt: nodes is a reading of the diagram manager rather than a count of the search, so a
    group can read differently every run with the search deciding the same thing each time. What
    the summary owes a reader is that the movement is visible and measured against a threshold.
    """

    def test_a_steady_measurement_says_so(self):
        run = {368: {"menu_ms": 40, "nodes": 39_324}, 602: {"menu_ms": 55, "nodes": 18_060}}
        text = summary_of([run, run, run])
        self.assertIn("nodes: identical in every run for all 2 measured group(s)", text)

    def test_a_group_that_moves_is_named_with_its_spread(self):
        text = summary_of(
            [
                {368: {"menu_ms": 40, "nodes": 39_324}, 602: {"menu_ms": 55, "nodes": 18_060}},
                {368: {"menu_ms": 41, "nodes": 39_412}, 602: {"menu_ms": 56, "nodes": 18_060}},
                {368: {"menu_ms": 40, "nodes": 39_367}, 602: {"menu_ms": 55, "nodes": 18_060}},
            ]
        )
        self.assertIn("groups whose nodes move between runs: 1 of 2 measured", text)
        self.assertIn("39,324 - 39,412", text)
        self.assertIn("(0.2%)", text)
        self.assertNotIn("602  18,060", text)

    def test_movement_is_counted_against_the_threshold(self):
        """A spread at or over NODES_NOISE counts, and one under it is reported but not counted."""
        wide = round(1000 * (1 + menus.NODES_NOISE) + 1)
        text = summary_of(
            [
                {368: {"menu_ms": 40, "nodes": 1000}, 602: {"menu_ms": 55, "nodes": 5000}},
                {368: {"menu_ms": 40, "nodes": wide}, 602: {"menu_ms": 55, "nodes": 5001}},
            ]
        )
        self.assertIn("2 of 2 measured, 1 of them by 1% or more", text)

    def test_the_marks_are_judged_separately_from_the_cost(self):
        """A group whose nodes move is still steady: a cost moving is not a decision moving."""
        text = summary_of(
            [
                {368: {"menu_ms": 40, "nodes": 39_324, "starred": "31,73"}},
                {368: {"menu_ms": 41, "nodes": 39_412, "starred": "31,73"}},
            ]
        )
        self.assertIn("groups whose rounds, settled or starred differ between runs: none", text)
        self.assertIn("groups whose nodes move between runs: 1 of 1 measured", text)


if __name__ == "__main__":
    unittest.main()
