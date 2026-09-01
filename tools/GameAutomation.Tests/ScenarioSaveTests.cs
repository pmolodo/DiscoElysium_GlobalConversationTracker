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
    /// <para>The three scenarios are sparse diffs of testing/save_template.ntwtf, and
    /// they are meant to differ in exactly one field: the money. Three of the four
    /// changed fields - where the player stands, which area they are in, and the two
    /// Siileng variables - are deliberately identical, because a scenario that moved the
    /// player somewhere slightly different would not be testing the same thing as its
    /// neighbours, and nothing about a run would say so.</para>
    ///
    /// <para>The tool cannot express that: a sparse diff must name one complete base, so
    /// the shared fields cannot be factored into an intermediate the three share. They
    /// are therefore three copies, hand-maintained, and this is what stops them
    /// drifting.</para>
    ///
    /// <para>It runs in the ordinary suite, with no game and no launch: it reads
    /// committed JSON.</para>
    /// </remarks>
    public class ScenarioSaveTests
    {
        private static string ScenarioRoot =>
            Path.Combine(GameInstall.RepoRoot(), "testing", "scenarios");

        private static readonly string[] Names =
        {
            "afford-both", "afford-only-sneakers", "afford-neither",
        };

        private static string Read(string scenario, string relative)
        {
            return File.ReadAllText(
                Path.Combine(ScenarioRoot, scenario + ".ntwtf", relative));
        }

        /// <summary>The money each scenario sets, from its own diff.</summary>
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

        [Fact]
        public void EveryScenarioIsThere()
        {
            foreach (string name in Names)
            {
                Assert.True(
                    Directory.Exists(Path.Combine(ScenarioRoot, name + ".ntwtf")),
                    $"{name} is missing from {ScenarioRoot}");
            }
        }

        [Fact]
        public void TheScenariosAgreeOnWhereThePlayerStands()
        {
            string[] actors = Names
                .Select(n => Read(n, $"{n}.ntwtf.lua.parts/Actor.json"))
                .ToArray();

            Assert.All(actors, actor => Assert.Equal(actors[0], actor));
            Assert.Contains("Position_Martinaise_ext", actors[0]);
        }

        [Fact]
        public void TheScenariosAgreeOnTheSiilengVariables()
        {
            string[] variables = Names
                .Select(n => Read(n, $"{n}.ntwtf.lua.parts/Variable.json"))
                .ToArray();

            Assert.All(variables, v => Assert.Equal(variables[0], v));
            Assert.Contains("jam.siileng_faln_sneakers", variables[0]);
            Assert.Contains("jam.siileng_learned_when_you_can_buy_speakers", variables[0]);
        }

        [Fact]
        public void TheScenariosAgreeOnTheArea()
        {
            string[] areas = Names.Select(n => Read(n, $"{n}.1st.ntwtf.json")).ToArray();

            Assert.All(areas, area => Assert.Equal(areas[0], area));
            Assert.Contains("Martinaise-ext", areas[0]);
        }

        [Fact]
        public void OnlyTheMoneyDiffers()
        {
            int[] money = Names.Select(MoneyOf).ToArray();

            Assert.Equal(money.Length, money.Distinct().Count());
        }

        [Fact]
        public void TheMoneyMatchesWhatTheRunExpects()
        {
            // The saves and the expectations are written in two places - the committed
            // diffs and Harness.LookAheadRun.Scenarios - and a run that disagreed with
            // its own fixtures would fail in the game, forty seconds in, saying only
            // that the balance was wrong.
            foreach (Harness.LookAheadScenario scenario in Harness.LookAheadRun.Scenarios)
            {
                Assert.Equal(scenario.Money, MoneyOf(scenario.SaveName));
            }
        }

        [Fact]
        public void EveryScenarioDiffsAgainstTheTemplate()
        {
            foreach (string name in Names)
            {
                using JsonDocument manifest = JsonDocument.Parse(
                    Read(name, "_archive.json"));
                string? relativeBase = manifest.RootElement
                    .GetProperty("base").GetString();

                Assert.NotNull(relativeBase);
                Assert.EndsWith("save_template.ntwtf", relativeBase!.Replace('\\', '/'));

                string resolved = Path.GetFullPath(Path.Combine(
                    ScenarioRoot, name + ".ntwtf", relativeBase!));
                Assert.True(
                    Directory.Exists(resolved),
                    $"{name}'s base does not resolve to a directory: {resolved}");
            }
        }

        [Fact]
        public void TheStagedGlobalStateLeavesTheOneEntryUnseen()
        {
            using JsonDocument state = JsonDocument.Parse(
                File.ReadAllText(
                    Path.Combine(ScenarioRoot, "global-conversation-state.json")));

            JsonElement conversation = state.RootElement
                .GetProperty("conversations")
                .GetProperty(Harness.LookAheadRun.ConversationId.ToString());

            // Entry 80 is the only one reachable exclusively through the speakers
            // purchase, so it is what an orange marker means. Marking it would make
            // every scenario pass for the wrong reason.
            Assert.False(
                conversation.TryGetProperty("80", out _),
                "entry 80 must stay unseen; it is what the marker is looking for");
            Assert.Equal(94, conversation.EnumerateObject().Count());
        }
    }
}
