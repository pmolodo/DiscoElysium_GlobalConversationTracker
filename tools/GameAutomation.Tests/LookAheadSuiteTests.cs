// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using GlobalConversationTracker.Harness;
using GlobalConversationTracker.Persistence;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// The suites are declarations, so what can be checked without a game is that they
    /// declare something coherent.
    /// </summary>
    /// <remarks>
    /// Worth checking because the cost of finding out in the game is a minute per suite,
    /// and every one of these mistakes - a save that is not committed, a suite whose
    /// global state does not exist, two suites answering to one name - fails a long way
    /// from its cause.
    /// </remarks>
    public class LookAheadSuiteTests
    {
        private static string ScenarioRoot =>
            Path.Combine(GameInstall.RepoRoot(), "testing", "scenarios");

        [Fact]
        public void ThereIsAtLeastOneSuite()
        {
            Assert.NotEmpty(LookAheadSuites.All);
        }

        /// <summary>
        /// The default run leaves out what is checked without a game, but the suite still
        /// exists, is still validated by the checks that walk every suite, and is still
        /// reachable by name.
        /// </summary>
        [Fact]
        public void AllSeenIsDeclaredButNotInTheDefaultRun()
        {
            Assert.Contains(LookAheadSuites.AllSeen, LookAheadSuites.All);
            Assert.DoesNotContain(LookAheadSuites.AllSeen, LookAheadSuites.Default);
            Assert.Equal(
                new[] { LookAheadSuites.AllSeen },
                LookAheadSuites.SelectMany(new[] { "all-seen" }));
        }

        [Fact]
        public void TheDefaultRunIsMadeOfDeclaredSuites()
        {
            Assert.All(LookAheadSuites.Default, suite => Assert.Contains(suite, LookAheadSuites.All));
            Assert.NotEmpty(LookAheadSuites.Default);
        }

        [Fact]
        public void NamingNoSuiteRunsTheDefaultSet()
        {
            Assert.Equal(LookAheadSuites.Default, LookAheadSuites.SelectMany(Array.Empty<string>()));
            Assert.Equal(LookAheadSuites.Default, LookAheadSuites.Select(null));
        }

        [Fact]
        public void SuiteNamesAreDistinct()
        {
            string[] names = LookAheadSuites.All.Select(s => s.Name).ToArray();

            Assert.Equal(names.Length, names.Distinct(StringComparer.OrdinalIgnoreCase).Count());
        }

        [Fact]
        public void EverySuiteNamesAGlobalStateThatExists()
        {
            foreach (LookAheadSuite suite in LookAheadSuites.All)
            {
                Assert.True(
                    File.Exists(Path.Combine(ScenarioRoot, suite.GlobalStateFile)),
                    $"suite '{suite.Name}' names a missing global state: {suite.GlobalStateFile}");
            }
        }

        /// <remarks>
        /// Existing is not enough - the mod has to be able to read it. A fixture in a
        /// shape the parser rejects loads as an empty state, and an empty state makes
        /// every option unseen-anywhere, which is a passing-looking run of the wrong
        /// experiment.
        /// </remarks>
        [Fact]
        public void EverySuitesGlobalStateIsReadableByTheMod()
        {
            foreach (LookAheadSuite suite in LookAheadSuites.All)
            {
                string path = Path.Combine(ScenarioRoot, suite.GlobalStateFile);
                GlobalStateLoadResult result = GlobalStateJson.Deserialize(
                    File.ReadAllBytes(path), path);

                Assert.True(
                    result.Outcome == GlobalStateLoadOutcome.Loaded,
                    $"suite '{suite.Name}' names a global state the mod would refuse: "
                        + $"{result.Outcome} - {result.ErrorMessage}");
                Assert.True(
                    result.SkippedRowCount == 0,
                    $"suite '{suite.Name}' names a global state with "
                        + $"{result.SkippedRowCount} unreadable row(s): "
                        + string.Join("; ", result.Warnings));
            }
        }

        [Fact]
        public void EveryScenarioNamesASaveThatExists()
        {
            foreach (LookAheadSuite suite in LookAheadSuites.All)
            {
                foreach (LookAheadScenario scenario in suite.Scenarios)
                {
                    Assert.True(
                        Directory.Exists(
                            Path.Combine(ScenarioRoot, scenario.SaveName + ".ntwtf")),
                        $"suite '{suite.Name}' names a missing save: {scenario.SaveName}");
                }
            }
        }

        [Fact]
        public void EverySuiteHasScenarios()
        {
            foreach (LookAheadSuite suite in LookAheadSuites.All)
            {
                Assert.NotEmpty(suite.Scenarios);
            }
        }

        [Fact]
        public void EveryScenarioSaysSomethingAboutSomeOption()
        {
            // A scenario with no expectations would launch the game, open a conversation
            // and assert nothing, which reads as a passing test.
            foreach (LookAheadSuite suite in LookAheadSuites.All)
            {
                foreach (LookAheadScenario scenario in suite.Scenarios)
                {
                    Assert.True(
                        scenario.Options.Count > 0
                            || scenario.Markers != MarkerPolicy.Named,
                        $"{suite.Name}/{scenario.SaveName} asserts nothing at all");
                }
            }
        }

        [Fact]
        public void PristineKeepsItsFourRepresentativeConversations()
        {
            Assert.Equal(
                new[] { 9, 717, 892, 28 },
                LookAheadSuites.Pristine.Scenarios.Select(s => s.ConversationId));
        }

        /// <summary>
        /// The suite formerly known as headroom claims a no-crawl, not a cost. Once
        /// every entry is recorded nothing can outrank an option, so the short-circuit
        /// skips every crawl - and a suite that still demanded a cost would fail on
        /// correct behaviour, which is exactly what it did before this changed.
        /// </summary>
        [Fact]
        public void AllSeenClaimsNoCrawlRatherThanACost()
        {
            LookAheadSuite suite = LookAheadSuites.AllSeen;
            SuiteArtefact statistics = Assert.Single(
                suite.Artefacts,
                artefact => artefact.FileName == "look-ahead-stats.json");

            // An absent file is the pristine case and passes; a file saying a state was
            // built is the failure. Asks are NOT the failure - the engine records one for
            // every option it is handed, including the ones it refuses to search - so the
            // fixture reads states rather than crawls. See LookAheadSuites.NoCrawls.
            Assert.Null(statistics.Check(null));
            Assert.Null(statistics.Check(Statistics(crawls: 4, states: 0)));
            Assert.NotNull(statistics.Check(Statistics(crawls: 1, states: 1)));
        }

        /// <summary>
        /// It puts the claim to five different conversations, and asks the mod to leave a
        /// trace so that "nothing happened" means something.
        /// </summary>
        /// <remarks>
        /// IT NO LONGER CHECKS WHICH FIVE, and that is not a gap. They used to be listed
        /// in C# beside the suite that used them, so a test comparing the two was
        /// comparing a definition with a copy of itself; the list now lives once, in
        /// <c>testing/scenarios/suites.json</c>. The claim those five were chosen for -
        /// that they are the largest conversations in the game, so a crawl would run here
        /// if it ran anywhere - is checked in <c>tests/all_seen.rs</c> against the shipped
        /// index, which is the only place it can be checked rather than restated.
        /// </remarks>
        [Fact]
        public void AllSeenAsksSeveralConversationsAndKeepsTheStatistics()
        {
            LookAheadSuite suite = LookAheadSuites.AllSeen;

            Assert.Equal(5, suite.Scenarios.Count);
            Assert.Equal(
                suite.Scenarios.Count,
                suite.Scenarios.Select(scenario => scenario.ConversationId).Distinct().Count());
            Assert.Equal("true", suite.PluginSettings["KeepLookAheadStates"]);
        }

        [Fact]
        public void SeenHereIsTheExhaustedNoCrawlFixture()
        {
            LookAheadSuite suite = LookAheadSuites.SeenHere;
            LookAheadScenario scenario = Assert.Single(suite.Scenarios);
            SuiteArtefact statistics = Assert.Single(
                suite.Artefacts,
                artefact => artefact.FileName == "look-ahead-stats.json");

            Assert.Equal("global-state-all-seen-elsewhere.json", suite.GlobalStateFile);
            Assert.Equal("seen-here-all", scenario.SaveName);
            Assert.Equal(MarkerPolicy.Named, scenario.Markers);
            Assert.Equal("true", suite.PluginSettings["KeepLookAheadStates"]);
            Assert.Null(statistics.Check(null));
            Assert.NotNull(statistics.Check(Statistics(crawls: 1, states: 1)));
        }

        [Fact]
        public void NoScenarioNamesTheSameEntryTwice()
        {
            foreach (LookAheadSuite suite in LookAheadSuites.All)
            {
                foreach (LookAheadScenario scenario in suite.Scenarios)
                {
                    int[] entries = scenario.Options.Select(o => o.EntryId).ToArray();
                    Assert.Equal(
                        entries.Length,
                        entries.Distinct().Count());
                }
            }
        }

        /// <remarks>
        /// The bug this pins down cost four silent failures. Only the newest save on
        /// disk can be loaded by the Continue press that starts a run, so the first
        /// scenario's save has to be packed last - and a save that several suites share
        /// used to be positioned by its last use rather than its first, which put
        /// afford-both fifth of twelve. Continue then loaded a different save, and the
        /// money scenario measured a balance nobody asked for while still drawing a menu
        /// and reporting on it.
        /// </remarks>
        [Fact]
        public void TheFirstScenariosSaveIsStagedLastSoItIsTheNewest()
        {
            IReadOnlyList<LookAheadSuite> all = LookAheadSuites.All;
            IReadOnlyList<string> order = LookAheadRun.StagingOrder(all);

            Assert.Equal(all[0].Scenarios[0].SaveName, order[order.Count - 1]);
        }

        [Fact]
        public void EverySaveIsStagedExactlyOnceHoweverManySuitesShareIt()
        {
            IReadOnlyList<string> order = LookAheadRun.StagingOrder(LookAheadSuites.All);

            Assert.Equal(order.Count, order.Distinct(StringComparer.Ordinal).Count());
            Assert.Equal(
                LookAheadSuites.All
                    .SelectMany(suite => suite.Scenarios)
                    .Select(scenario => scenario.SaveName)
                    .Distinct(StringComparer.Ordinal)
                    .Count(),
                order.Count);
        }

        [Fact]
        public void ASharedSaveIsPlacedByItsFirstUseNotItsLast()
        {
            // Two suites, the second reusing the first's save. Staging must still end
            // with the first suite's first save.
            LookAheadSuite[] suites = { LookAheadSuites.Money, LookAheadSuites.SwitchedOff };

            IReadOnlyList<string> order = LookAheadRun.StagingOrder(suites);

            Assert.Equal(LookAheadSuites.Money.Scenarios[0].SaveName, order[order.Count - 1]);
        }

        [Fact]
        public void SelectingBySuiteNameFindsIt()
        {
            IReadOnlyList<LookAheadSuite> selected = LookAheadSuites.Select("money");

            Assert.Single(selected);
            Assert.Equal("money", selected[0].Name);
        }

        /// <summary>
        /// Selecting nothing runs the DEFAULT set, which is no longer every suite: one
        /// whose claim is checked without a game is left out of the in-game run.
        /// </summary>
        [Fact]
        public void SelectingNothingRunsTheDefaultSet()
        {
            Assert.Equal(LookAheadSuites.Default.Count, LookAheadSuites.Select(null).Count);
            Assert.True(
                LookAheadSuites.Default.Count < LookAheadSuites.All.Count,
                "the default set should be a strict subset, or nothing has been moved offline");
        }

        [Fact]
        public void SelectingMultipleNamesPreservesTheirOrder()
        {
            IReadOnlyList<LookAheadSuite> selected = LookAheadSuites.SelectMany(
                new[] { "pristine", "budget" });

            Assert.Equal(new[] { "pristine", "budget" }, selected.Select(suite => suite.Name));
        }

        [Fact]
        public void AnUnknownSuiteNameIsRefusedAndListsTheRealOnes()
        {
            ArgumentException error = Assert.Throws<ArgumentException>(
                () => LookAheadSuites.Select("no-such-suite"));

            Assert.Contains("no-such-suite", error.Message);
            Assert.Contains("money", error.Message);
        }

        [Fact]
        public void AScenarioKnowsWhichEntriesItNames()
        {
            LookAheadScenario scenario = LookAheadSuites.Money.Scenarios[0];

            Assert.True(scenario.Names(LookAheadSuites.BuySneakersEntry));
            Assert.False(scenario.Names(LookAheadSuites.SpeakersOnlyEntry));

            // An option whose entry the probe could not read is named by nothing.
            Assert.False(scenario.Names(null));
        }

        [Fact]
        public void NamingNoScenarioKeepsEverySuiteWhole()
        {
            IReadOnlyList<LookAheadSuite> suites = LookAheadSuites.Default;

            Assert.Same(suites, LookAheadSuites.Only(suites, Array.Empty<string>()));
        }

        /// <summary>
        /// A bare save name takes every scenario that loads it.
        /// </summary>
        /// <remarks>
        /// The pristine suite opens four different conversations from one save, so this is
        /// the case a save name alone cannot narrow - and must not pretend to.
        /// </remarks>
        [Fact]
        public void ASaveNameTakesEveryScenarioThatLoadsIt()
        {
            IReadOnlyList<LookAheadSuite> only = LookAheadSuites.Only(
                new[] { LookAheadSuites.Pristine },
                new[] { LookAheadSuites.CeilingFan.Save });

            LookAheadSuite suite = Assert.Single(only);
            Assert.All(
                suite.Scenarios,
                scenario => Assert.Equal(LookAheadSuites.CeilingFan.Save, scenario.SaveName));
            Assert.NotEmpty(suite.Scenarios);
        }

        [Fact]
        public void ASaveAndAConversationTakeExactlyOne()
        {
            IReadOnlyList<LookAheadSuite> only = LookAheadSuites.Only(
                new[] { LookAheadSuites.Pristine },
                new[]
                {
                    LookAheadSuites.CeilingFan.Save + ":" + LookAheadSuites.CeilingFan.Conversation,
                });

            LookAheadScenario scenario = Assert.Single(Assert.Single(only).Scenarios);
            Assert.Equal(LookAheadSuites.CeilingFan.Conversation, scenario.ConversationId);
        }

        /// <summary>
        /// A suite left with nothing drops out, rather than running as an empty one.
        /// </summary>
        [Fact]
        public void ASuiteWithNoMatchingScenarioIsNotRun()
        {
            IReadOnlyList<LookAheadSuite> only = LookAheadSuites.Only(
                new[] { LookAheadSuites.Pristine, LookAheadSuites.Money },
                new[]
                {
                    LookAheadSuites.CeilingFan.Save + ":" + LookAheadSuites.CeilingFan.Conversation,
                });

            Assert.Equal("pristine", Assert.Single(only).Name);
        }

        /// <summary>
        /// A name matching nothing is refused, and the error says what there was.
        /// </summary>
        /// <remarks>
        /// The whole purpose of the filter is to run less, so a typo that ran nothing and
        /// reported "0/0 passed" would be the worst thing it could do - and the most
        /// believable, since a filtered run is expected to be short.
        /// </remarks>
        [Fact]
        public void AScenarioNameThatMatchesNothingIsRefused()
        {
            ArgumentException error = Assert.Throws<ArgumentException>(
                () => LookAheadSuites.Only(
                    new[] { LookAheadSuites.Pristine }, new[] { "at-the-moon" }));

            Assert.Contains("at-the-moon", error.Message);
            Assert.Contains(LookAheadSuites.CeilingFan.Save, error.Message);
        }

        /// <summary>
        /// A filtered suite says so, and makes none of its whole-suite claims.
        /// </summary>
        /// <remarks>
        /// Its artefacts and log expectations are about the complete set of scenarios - a
        /// statistics file written over all of them, an overflow log naming one of them -
        /// so keeping them under a filter would fail a run that is behaving perfectly.
        /// </remarks>
        [Fact]
        public void AFilteredSuiteDropsItsWholeSuiteChecks()
        {
            Assert.NotEmpty(LookAheadSuites.Budget.Artefacts);

            LookAheadSuite only = Assert.Single(LookAheadSuites.Only(
                new[] { LookAheadSuites.Budget },
                new[] { LookAheadSuites.Siileng.Save }));

            Assert.True(only.Filtered);
            Assert.Empty(only.Artefacts);
            Assert.Empty(only.LogExpectations);

            // What makes a scenario mean what it means comes along.
            Assert.Equal(LookAheadSuites.Budget.GlobalStateFile, only.GlobalStateFile);
            Assert.Equal(LookAheadSuites.Budget.PluginSettings, only.PluginSettings);
            Assert.False(LookAheadSuites.Budget.Filtered);
        }

        /// <summary>The part of a statistics file the no-crawl fixture reads.</summary>
        private static string Statistics(int crawls, int states) =>
            $"{{\"crawls\":{crawls},\"states\":{{\"total\":{states}}}}}";
    }
}
