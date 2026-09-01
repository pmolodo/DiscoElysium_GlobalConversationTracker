// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using GlobalConversationTracker.Harness;
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

        [Fact]
        public void SelectingBySuiteNameFindsIt()
        {
            IReadOnlyList<LookAheadSuite> selected = LookAheadSuites.Select("money");

            Assert.Single(selected);
            Assert.Equal("money", selected[0].Name);
        }

        [Fact]
        public void SelectingNothingRunsEverySuite()
        {
            Assert.Equal(LookAheadSuites.All.Count, LookAheadSuites.Select(null).Count);
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
    }
}
