// SPDX-License-Identifier: MIT
using System.IO;
using System.Linq;
using System.Text.Json;
using GlobalConversationTracker.Core;
using GlobalConversationTracker.Persistence;
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

        /// <summary>
        /// The money suite's scenarios, read from the DEFINITION rather than from a built
        /// suite.
        /// </summary>
        /// <remarks>
        /// THE SAVES ARE COMMITTED WHETHER OR NOT THE SUITE RUNS, and what these tests check
        /// is the saves - that the diffs carry the balances the definition names. The suite
        /// is switched off while the symbolic search cannot model money (see the `disabled`
        /// sentence in suites.json), and its fixtures must not rot in the meantime, so this
        /// reads the table directly and keeps checking them.
        /// </remarks>
        private static Harness.ScenarioSuiteDefinition MoneySuite =>
            Harness.ScenarioTable.Read().Suites.Single(suite => suite.Suite == "money");

        private static string[] Scenarios =>
            MoneySuite.Scenarios.Select(s => s.Save).ToArray();

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
            // the money suite's definition - and a run that disagreed with its own fixtures
            // would fail in the game, a minute in, saying only that the balance was wrong.
            foreach (Harness.ScenarioDefinition scenario in MoneySuite.Scenarios)
            {
                Assert.Equal(scenario.Money, MoneyOf(scenario.Save));
            }
        }

        [Fact]
        public void TheScenariosDifferFromEachOther()
        {
            int[] money = Scenarios.Select(MoneyOf).ToArray();

            Assert.Equal(money.Length, money.Distinct().Count());
        }

        /// <remarks>
        /// Read through the mod's own reader rather than by walking the raw JSON. The
        /// file's shape is a storage detail that has already changed once - it was a
        /// property per entry, and is now grouped by status - and a test that navigates
        /// the shape by hand fails on a migration that lost nothing, which is what
        /// happened here. What the suite depends on is which entries are recorded, and
        /// the reader answers that in either format.
        /// </remarks>
        [Fact]
        public void TheStagedGlobalStateLeavesTheOneEntryUnseen()
        {
            string path = Path.Combine(ScenarioRoot, "global-conversation-state.json");
            GlobalStateLoadResult loaded = GlobalStateJson.Deserialize(
                File.ReadAllBytes(path), path);

            Assert.Equal(GlobalStateLoadOutcome.Loaded, loaded.Outcome);
            GlobalConversationState state = loaded.RequireState();

            // Entry 80 is the only one reachable exclusively through the speakers
            // purchase, so it is what an orange marker means. Marking it would make every
            // scenario pass for the wrong reason.
            Assert.False(
                state.TryGetStatus(
                    Harness.LookAheadSuites.SiilengConversation,
                    Harness.LookAheadSuites.SpeakersOnlyEntry,
                    out _),
                "entry 80 must stay unseen; it is what the marker is looking for");
            Assert.Equal(
                94,
                state.GetConversationEntries(
                    Harness.LookAheadSuites.SiilengConversation).Count());
        }
    }
}
