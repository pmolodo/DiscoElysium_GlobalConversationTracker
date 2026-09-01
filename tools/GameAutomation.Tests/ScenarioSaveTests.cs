// SPDX-License-Identifier: MIT
using System.IO;
using System.Linq;
using System.Text.Json;
using Xunit;

namespace GlobalConversationTracker.Automation.Tests
{
    /// <summary>
    /// The committed look-ahead scenarios say what they are meant to say.
    /// </summary>
    /// <remarks>
    /// <para>The layout is the documentation. at-siileng.ntwtf is a diff of
    /// save_template carrying the setup all three scenarios share - the area, where the
    /// player stands, the two Siileng variables - and each scenario is a diff of THAT
    /// carrying one field, its money. A reader can see which field varies without
    /// comparing anything.</para>
    ///
    /// <para>What is left to check is that the structure is still that shape, and that
    /// the money in the files matches the money the run expects, which is written
    /// somewhere else entirely. It reads committed JSON, so it runs in the ordinary
    /// suite with no game and no launch.</para>
    /// </remarks>
    public class ScenarioSaveTests
    {
        /// <summary>The diff every scenario is built on.</summary>
        private const string Shared = "at-siileng";

        private static string ScenarioRoot =>
            Path.Combine(GameInstall.RepoRoot(), "testing", "scenarios");

        private static string Read(string save, string relative) =>
            File.ReadAllText(Path.Combine(ScenarioRoot, save + ".ntwtf", relative));

        private static JsonElement Manifest(string save)
        {
            using JsonDocument document = JsonDocument.Parse(Read(save, "_archive.json"));
            return document.RootElement.Clone();
        }

        private static int MoneyOf(string scenario)
        {
            using JsonDocument document = JsonDocument.Parse(
                Read(scenario, $"{scenario}.2nd.ntwtf.json"));
            return document.RootElement
                .GetProperty("_changes")
                .GetProperty("playerCharacter")
                .GetProperty("Money")
                .GetInt32();
        }

        private static string[] Scenarios =>
            Harness.LookAheadSuites.Money.Scenarios.Select(s => s.SaveName).ToArray();

        [Fact]
        public void TheSharedSetupCarriesEverythingButTheMoney()
        {
            string parts = $"{Shared}.ntwtf.lua.parts";

            Assert.Contains("Martinaise-ext", Read(Shared, $"{Shared}.1st.ntwtf.json"));
            Assert.Contains(
                "Position_Martinaise_ext", Read(Shared, $"{parts}/Actor.json"));
            Assert.Contains(
                "jam.siileng_faln_sneakers", Read(Shared, $"{parts}/Variable.json"));
            Assert.Contains(
                "jam.siileng_learned_when_you_can_buy_speakers",
                Read(Shared, $"{parts}/Variable.json"));
        }

        [Fact]
        public void TheSharedSetupDiffsAgainstTheTemplate()
        {
            string? relativeBase = Manifest(Shared).GetProperty("base").GetString();

            Assert.EndsWith(
                "save_template.ntwtf", relativeBase!.Replace('\\', '/'));
            Assert.True(Directory.Exists(Path.GetFullPath(
                Path.Combine(ScenarioRoot, Shared + ".ntwtf", relativeBase!))));
        }

        [Fact]
        public void EveryScenarioDiffsAgainstTheSharedSetup()
        {
            foreach (string scenario in Scenarios)
            {
                string? relativeBase = Manifest(scenario).GetProperty("base").GetString();

                Assert.EndsWith(
                    Shared + ".ntwtf", relativeBase!.Replace('\\', '/'));
            }
        }

        [Fact]
        public void EveryScenarioChangesNothingButItsMoney()
        {
            // The invariant the layout exists to make obvious. A scenario that carried a
            // second file would be varying something the others do not, and no run would
            // say so.
            foreach (string scenario in Scenarios)
            {
                string[] members = Directory
                    .GetFiles(Path.Combine(ScenarioRoot, scenario + ".ntwtf"))
                    .Select(Path.GetFileName)
                    .Where(name => name != "_archive.json")
                    .ToArray()!;

                Assert.Equal(new[] { $"{scenario}.2nd.ntwtf.json" }, members);

                // Not even an empty one. Money lives in the 2nd JSON, so a scenario
                // changes no Lua table, and the base it inherits them from is named by
                // _archive.json rather than by a second file down here.
                Assert.False(Directory.Exists(Path.Combine(
                    ScenarioRoot, scenario + ".ntwtf", $"{scenario}.ntwtf.lua.parts")));
            }
        }

        [Fact]
        public void TheMoneyMatchesWhatTheRunExpects()
        {
            // The saves and the expectations live in two places - the committed diffs and
            // Harness.LookAheadSuites.Money.Scenarios - and a run that disagreed with its own
            // fixtures would fail in the game, a minute in, saying only that the balance
            // was wrong.
            foreach (Harness.LookAheadScenario scenario in Harness.LookAheadSuites.Money.Scenarios)
            {
                Assert.Equal(scenario.Money, MoneyOf(scenario.SaveName));
            }
        }

        [Fact]
        public void TheScenariosDifferFromEachOther()
        {
            int[] money = Scenarios.Select(MoneyOf).ToArray();

            Assert.Equal(money.Length, money.Distinct().Count());
        }

        [Fact]
        public void TheStagedGlobalStateLeavesTheOneEntryUnseen()
        {
            using JsonDocument state = JsonDocument.Parse(
                File.ReadAllText(
                    Path.Combine(ScenarioRoot, "global-conversation-state.json")));

            JsonElement conversation = state.RootElement
                .GetProperty("conversations")
                .GetProperty(Harness.LookAheadSuites.SiilengConversation.ToString());

            // Entry 80 is the only one reachable exclusively through the speakers
            // purchase, so it is what an orange marker means. Marking it would make every
            // scenario pass for the wrong reason.
            Assert.False(
                conversation.TryGetProperty(
                    Harness.LookAheadSuites.SpeakersOnlyEntry.ToString(), out _),
                "entry 80 must stay unseen; it is what the marker is looking for");
            Assert.Equal(94, conversation.EnumerateObject().Count());
        }
    }
}
