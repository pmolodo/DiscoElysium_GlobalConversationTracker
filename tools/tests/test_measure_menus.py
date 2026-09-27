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


class OptionsReachTheRun(unittest.TestCase):
    """What the command line says is what the run uses.

    WORTH A TEST BECAUSE THE FAILURE IS SILENT. An option that is parsed and then not consulted
    leaves a run that measures under the defaults while its own help, and its run record's command
    line, say otherwise - and every number it produces looks exactly like a number taken the way
    it was asked for.
    """

    def parse(self, *argv):
        return menus.get_parser().parse_args(argv)

    def test_the_settle_rule_is_the_one_asked_for(self):
        args = self.parse("--settle-groups", "3", "--settle-factor", "5", "--settle-ms", "7", "368")
        settle = menus.Settling.of(args)
        self.assertEqual((settle.groups, settle.factor, settle.ms), (3, 5, 7))

    def test_the_settle_rule_defaults_where_nothing_is_asked(self):
        settle = menus.Settling.of(self.parse("368"))
        self.assertEqual(settle.ms, menus.SETTLE_MS)
        self.assertGreater(settle.groups, 0)

    def test_the_rule_a_run_prints_is_the_rule_it_was_given(self):
        """The sentence in the log and in run.json is built from the same object the phase uses."""
        settle = menus.Settling.of(self.parse("--settle-groups", "3", "--settle-ms", "7", "368"))
        self.assertIn("3 in a row", settle.rule())
        self.assertIn("7ms", settle.rule())

    def test_a_named_worker_count_is_not_second_guessed(self):
        """Including upwards, and including 1 on a machine with many cores."""
        self.assertEqual(self.parse("--workers", "1", "368").workers, 1)
        self.assertEqual(self.parse("--workers", "64", "368").workers, 64)

    def test_nothing_named_leaves_the_machine_to_answer(self):
        """None rather than a number, so `default_workers` is what decides."""
        self.assertIsNone(self.parse("368").workers)

    def test_a_run_that_names_no_arm_asks_for_none(self):
        """So the driver's own default is what a default run measures."""
        self.assertEqual(menus.driver_arguments(None, []), [])

    def test_the_arm_reaches_the_driver(self):
        self.assertEqual(menus.driver_arguments("onward", []), ["--marking", "onward"])

    def test_pass_through_arguments_travel_with_the_arm(self):
        self.assertEqual(
            menus.driver_arguments("onward", ["--starts", "3"]),
            ["--marking", "onward", "--starts", "3"],
        )

    def test_pass_through_arguments_are_split_the_way_a_shell_would(self):
        """So a quoted value with a space in it survives, whichever shell was in the way."""
        parsed = self.parse("--driver", "--nolimit --save 'a name with spaces'", "368")
        self.assertEqual(
            menus.shlex.split(parsed.driver),
            ["--nolimit", "--save", "a name with spaces"],
        )

    def test_a_single_pass_through_option_needs_the_equals_form(self):
        """Attached it parses; separate, argparse reads it as an option of this tool and refuses.

        WORTH A TEST BECAUSE THE HELP HAD TO LEARN IT THE HARD WAY. `--driver "--nolimit --starts
        24"` works - argparse takes a value with a space in it - and `--driver "--no-cache"` does
        not, which makes the failure look arbitrary. It is the one shape a caller reaches for
        most: one flag, passed straight through.
        """
        self.assertEqual(self.parse("--driver=--no-cache", "368").driver, "--no-cache")
        with self.assertRaises(SystemExit):
            with redirect_stderr(StringIO()):
                self.parse("--driver", "--no-cache", "368")


class ProgressLines(unittest.TestCase):
    """A progress line says where it is in the whole pass, not only in the run it belongs to."""

    def test_a_prefixed_line_reads_from_the_outside_in(self):
        """The caller's prefix, then the run's, then the count - the line an arm of four reads."""
        line = menus.common.progress_line(
            427,
            429,
            "conversation 1150",
            note="nothing to measure",
            prefix="[Arm 1/4][Run 2/4]",
            unit="Conv",
        )
        self.assertEqual(line, "[Arm 1/4][Run 2/4][Conv 427/429  99%] conversation 1150  nothing to measure")

    def test_a_line_asked_for_nothing_more_is_as_it_was(self):
        line = menus.common.progress_line(5, 429, "conversation 7", note="20 ms")
        self.assertEqual(line, "[  5/429   1%] conversation 7  20 ms")

    def test_the_status_prefix_reaches_the_run(self):
        """Parsed, and empty where nothing is asked, so a plain run's lines start at the run."""
        parse = menus.get_parser().parse_args
        self.assertEqual(parse(["--status-prefix=[Arm 1/4]", "368"]).status_prefix, "[Arm 1/4]")
        self.assertEqual(parse(["368"]).status_prefix, "")


class ForeignLoadSums(unittest.TestCase):
    """What else used the machine is worked out from two readings the way the docstring says."""

    def test_shares_are_of_the_whole_machine_and_count_only_what_is_known(self):
        """8 cores over 10 s is 80 CPU seconds, and three processes' growth is counted.

        a grows 8 s and b 1 s; c started since the first reading, so all 4 of its seconds are
        this interval's; d was there before but not read then, so its time is not known and it
        is left out.
        """
        readings = iter(
            [
                (100.0, 0.0, {(1, 50.0): ("a", 10.0), (2, 50.0): ("b", 1.0)}),
                (
                    110.0,
                    80.0,
                    {
                        (1, 50.0): ("a", 18.0),
                        (2, 50.0): ("b", 2.0),
                        (3, 105.0): ("c", 4.0),
                        (4, 90.0): ("d", 5.0),
                    },
                ),
            ]
        )
        load = menus.common.ForeignLoad()
        load._snapshot = lambda: next(readings)
        load._poll()
        load._poll()
        found = load.summary()
        self.assertEqual((found["percent"], found["highest_percent"]), (16.2, 16.2))
        self.assertEqual(
            found["busiest"],
            [{"name": "a", "percent": 10.0}, {"name": "c", "percent": 5.0}, {"name": "b", "percent": 1.2}],
        )

    def test_the_line_names_the_busiest(self):
        line = menus.common.load_line(
            {
                "percent": 12.3,
                "highest_percent": 41.0,
                "busiest": [{"name": "msmpeng", "percent": 5.1}, {"name": "dwm", "percent": 3.2}],
                "interval_s": 5.0,
            }
        )
        self.assertEqual(
            line,
            "other processes: 12.3% of the machine on average, 41.0% at most over one 5.0s poll; "
            "busiest: msmpeng 5.1%, dwm 3.2%",
        )


class Arms(unittest.TestCase):
    """Several arms in one invocation: everything from one `--arm` to the next, unquoted."""

    def test_no_arm_named_is_one_arm_of_no_name(self):
        """So a run that names none goes through the same loop as one that names four."""
        self.assertEqual(menus.split_arms(["--runs", "3", "all"]), (["--runs", "3", "all"], [(None, [])]))

    def test_an_arm_is_its_name_and_the_words_up_to_the_next(self):
        """The tool's own arguments come first; an arm's driver options are never read as them."""
        own, arms = menus.split_arms(
            [
                "--runs",
                "3",
                "all",
                "--arm",
                "shipped-defaults",
                "--arm",
                "first-link",
                "--menu",
                "first",
                "--targets",
                "link-deepest",
            ]
        )
        self.assertEqual(own, ["--runs", "3", "all"])
        self.assertEqual(
            arms,
            [
                ("shipped-defaults", []),
                ("first-link", ["--menu", "first", "--targets", "link-deepest"]),
            ],
        )

    def test_every_form_argparse_takes_is_an_arm(self):
        """`--arm=name` as well as `--arm name`, and the two mixed on one line."""
        self.assertEqual(
            menus.split_arms(["all", "--arm=a", "--nolimit", "--arm", "b", "--menu", "first"]),
            (["all"], [("a", ["--nolimit"]), ("b", ["--menu", "first"])]),
        )

    def test_what_is_left_parses_as_the_tools_own(self):
        own, _ = menus.split_arms(["--workers", "1", "368", "--arm", "a", "--nolimit"])
        parsed = menus.get_parser().parse_args(own)
        self.assertEqual((parsed.workers, parsed.conversations), (1, ["368"]))

    def test_an_arm_with_no_name_is_refused(self):
        """`--arm --menu first` has lost its name, and would otherwise name a folder `--menu`."""
        with redirect_stderr(StringIO()), self.assertRaises(SystemExit):
            menus.split_arms(["all", "--arm", "--menu", "first"])
        with redirect_stderr(StringIO()), self.assertRaises(SystemExit):
            menus.split_arms(["all", "--arm"])

    def test_an_arm_named_twice_is_refused(self):
        """Two arms under one name would write into one folder."""
        with redirect_stderr(StringIO()), self.assertRaises(SystemExit):
            menus.split_arms(["all", "--arm", "a", "--arm", "a", "--nolimit"])


class CheapRun:
    """A pass whose every group costs the same few milliseconds, so it settles on the rule alone."""

    def columns(self):
        return HEADER

    def measure(self, conversation):
        return TAB.join(str(value) for value in [conversation, 1, 8, 8, 20, 100, 1, 8, ""]) + "\n", ""


class EveryPassSettlesOnItsOwn(unittest.TestCase):
    """A several-run measurement measures the heavy groups one at a time in EVERY run.

    WORTH A TEST BECAUSE THE FAILURE IS SILENT. One watch shared between passes arrives at the
    second already bottomed out, so every run after the cold one went parallel after its first
    group - measuring its heaviest groups side by side, in the runs whose medians are the result,
    while the log still printed the rule as if it had been followed.
    """

    def test_a_second_pass_on_the_same_rule_still_waits_for_the_run(self):
        rule = menus.Settling(3, 2, 0)
        groups = list(range(1, 20))
        with redirect_stdout(StringIO()):
            first = menus.serial_phase(CheapRun(), groups, {}, 4, rule, lambda *_: None)
            second = menus.serial_phase(CheapRun(), groups, {}, 4, rule, lambda *_: None)
        self.assertEqual((first, second), (3, 3))


if __name__ == "__main__":
    unittest.main()
