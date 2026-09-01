// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;

namespace GlobalConversationTracker.Harness
{
    /// <summary>The look-ahead suites this harness knows how to run.</summary>
    /// <remarks>
    /// Data rather than code. Each suite names a global state, a mod configuration and a
    /// list of saves, and every one of them runs through the same launch-load-open-read
    /// path; a new behaviour to cover is a new entry here plus the scenario save it
    /// names, not another copy of the run.
    /// </remarks>
    public static class LookAheadSuites
    {
        /// <summary>Siileng's stall, where a 0.50 purchase sits behind a 50.00 one.</summary>
        public const int SiilengConversation = 451;

        /// <summary>
        /// The entry only a speaker buyer reaches, left unseen so that reaching it is
        /// what an orange marker means.
        /// </summary>
        public const int SpeakersOnlyEntry = 80;

        /// <summary>The option that buys the sneakers, for 50.00.</summary>
        public const int BuySneakersEntry = 86;

        /// <summary>Two options that only look at the goods, and reach the hub again.</summary>
        public const int InspectSneakersEntry = 33;

        /// <summary>The other of the pair.</summary>
        public const int InspectSpeakersEntry = 67;

        /// <summary>The option that leaves, reaching nothing.</summary>
        public const int LeaveEntry = 85;

        /// <summary>The global state all three money scenarios share.</summary>
        private const string MoneyState = "global-conversation-state.json";

        /// <summary>Every suite, in the order a full run does them.</summary>
        /// <remarks>
        /// Computed rather than stored: a static field would be initialised before the
        /// suites it names, and would quietly hold nulls.
        /// </remarks>
        public static IReadOnlyList<LookAheadSuite> All =>
            new[] { Money, Budget, SwitchedOff };

        /// <summary>
        /// The forward scan spends as it walks.
        /// </summary>
        /// <remarks>
        /// Conversation 451 gates a 0.50 real purchase behind a 50.00 one, and the staged
        /// global state leaves entry 80 - the only one a speaker buyer reaches - unseen.
        /// An option is therefore orange exactly when the crawl could afford both, which
        /// is what the three balances separate. The middle one is the point: a scan that
        /// checked an option's price without subtracting what the path already spent
        /// would mark it.
        /// </remarks>
        public static LookAheadSuite Money { get; } = new LookAheadSuite(
            "money",
            "the forward scan spends as it walks",
            MoneyState,
            new[]
            {
                new LookAheadScenario(
                    "afford-both",
                    SiilengConversation,
                    "100 centimes left after the sneakers, so the speakers are still affordable",
                    new[]
                    {
                        Orange(BuySneakersEntry, "buying the sneakers leads on to the speakers"),
                        Orange(InspectSneakersEntry, "looking returns to the hub, which still can"),
                        Orange(InspectSpeakersEntry, "and so does looking at the other"),
                        Unmarked(LeaveEntry, "leaving reaches nothing at all"),
                    },
                    money: 5100),
                new LookAheadScenario(
                    "afford-only-sneakers",
                    SiilengConversation,
                    "25 centimes left after the sneakers, so the speakers are not affordable",
                    AllUnmarked("the speakers are out of reach once the sneakers are paid for"),
                    money: 5025),
                new LookAheadScenario(
                    "afford-neither",
                    SiilengConversation,
                    "the sneakers cannot be bought at all",
                    AllUnmarked("nothing on the path is affordable"),
                    money: 4900),
            },
            pluginSettings: new Dictionary<string, string>
            {
                // Costs this suite nothing - it makes the mod write a summary it would
                // otherwise keep to itself - and this is the suite with the most crawls
                // to summarise, so it is the cheapest place to check the summary is
                // right rather than paying for another launch.
                ["KeepLookAheadStates"] = "true",
            },
            artefacts: new[]
            {
                new SuiteArtefact(
                    "look-ahead-stats.json",
                    "the statistics account for every crawl",
                    CheckStatistics),
            });

    /// <summary>
    /// Reads look-ahead-stats.json and checks it adds up.
    /// </summary>
    /// <remarks>
    /// The identity is the point. Every crawl ends at one of three answers, so the three
    /// found counts must sum to the crawl count; if they did not, some crawl reached a
    /// state the classification does not name, and no marker check would say so.
    /// </remarks>
    private static string? CheckStatistics(string json)
    {
        using JsonDocument document = JsonDocument.Parse(json);
        JsonElement root = document.RootElement;

        int crawls = root.GetProperty("crawls").GetInt32();
        if (crawls <= 0)
        {
            return "no crawl was recorded at all";
        }

        JsonElement found = root.GetProperty("found");
        int total = found.GetProperty("nothing").GetInt32()
            + found.GetProperty("unseenThisGame").GetInt32()
            + found.GetProperty("unseenAnyGame").GetInt32();
        if (total != crawls)
        {
            return $"{crawls} crawls but {total} classified";
        }

        foreach (JsonElement conversation in root.GetProperty("byConversation").EnumerateArray())
        {
            if (conversation.GetProperty("conversation").GetInt32() == SiilengConversation)
            {
                return null;
            }
        }

        return $"nothing recorded for conversation {SiilengConversation}";
    }

        /// <summary>
        /// A crawl that runs out of budget shows nothing, and says where it stopped.
        /// </summary>
        /// <remarks>
        /// <para>The same save and the same global state as the money suite's first
        /// scenario, which marks three options - with one setting changed. That makes it
        /// a clean discriminator: if the markers still appear, the budget is not being
        /// honoured; if they vanish for any other reason, the money suite would have
        /// caught it.</para>
        ///
        /// <para>A budget of one is exhausted before the search dequeues anything, so
        /// LookAheadResult.Best stays at SeenThisGame and no option can beat its own
        /// state. That is the feature's deliberate failure mode: it costs a marker rather
        /// than a slow menu, and until now nothing checked that it does.</para>
        ///
        /// <para>The overflow log is the only other evidence, since saying nothing is
        /// exactly what an exhausted crawl does. Writing it also exercises the traced
        /// re-walk - a second engine that only runs on an overflow, so that menus which
        /// stay within budget pay nothing for a report they will never produce.</para>
        /// </remarks>
        public static LookAheadSuite Budget { get; } = new LookAheadSuite(
            "budget",
            "a crawl that runs out of budget shows nothing and says where",
            MoneyState,
            new[]
            {
                new LookAheadScenario(
                    "afford-both",
                    SiilengConversation,
                    "the balance that marks three options, with a budget of one",
                    AllUnmarked("the crawl gave up before it could reach anything"),
                    money: 5100),
            },
            pluginSettings: new Dictionary<string, string>
            {
                ["LookAheadStateBudget"] = "1",
                ["LogLookAheadBudgetExceeded"] = "true",
            },
            artefacts: new[]
            {
                new SuiteArtefact(
                    "look-ahead-budget-overflows.log",
                    "the overflow log names the option that ran out",
                    text => text.Contains($"budget exhausted", StringComparison.Ordinal)
                        && text.Contains($"{SiilengConversation}:", StringComparison.Ordinal)
                        ? null
                        : "no overflow block for conversation "
                            + $"{SiilengConversation} in {text.Length} characters"),
            });

        /// <summary>
        /// Turning the feature off leaves the options alone and the tracking working.
        /// </summary>
        /// <remarks>
        /// <para>The setting is documented as leaving tracking unaffected, and nothing
        /// checked either half of that.</para>
        ///
        /// <para>Unmarked options are not enough on their own to show the switch worked:
        /// the budget suite produces exactly the same menu by starving the crawl instead.
        /// What separates them is whether the hook was installed at all, which the mod
        /// says once at load - so this suite asserts the look-ahead hook line is absent
        /// while the tracking hook line is still there.</para>
        /// </remarks>
        public static LookAheadSuite SwitchedOff { get; } = new LookAheadSuite(
            "switched-off",
            "MarkLookAhead=false marks nothing and leaves tracking alone",
            MoneyState,
            new[]
            {
                new LookAheadScenario(
                    "afford-both",
                    SiilengConversation,
                    "the balance that marks three options, with the feature switched off",
                    AllUnmarked("the look-ahead is not installed at all"),
                    money: 5100),
            },
            pluginSettings: new Dictionary<string, string>
            {
                ["MarkLookAhead"] = "false",
            },
            logExpectations: new[]
            {
                new LogExpectation(
                    "options that can still lead to unread text are marked with an asterisk",
                    false,
                    "the look-ahead hook was not installed"),
                new LogExpectation(
                    "dialogue statuses are being tracked",
                    true,
                    "tracking is unaffected by the switch"),
            });

        /// <summary>Finds a suite by name.</summary>
        /// <param name="name">The suite's name, or null for every suite.</param>
        /// <exception cref="ArgumentException">No suite goes by that name.</exception>
        public static IReadOnlyList<LookAheadSuite> Select(string? name)
        {
            if (name == null)
            {
                return All;
            }

            LookAheadSuite? found = All.FirstOrDefault(
                suite => string.Equals(suite.Name, name, StringComparison.OrdinalIgnoreCase));

            return found == null
                ? throw new ArgumentException(
                    $"No look-ahead suite called '{name}'. Known suites: "
                    + string.Join(", ", All.Select(s => s.Name)) + ".",
                    nameof(name))
                : new[] { found };
        }

        private static OptionExpectation Orange(int entryId, string why) =>
            new OptionExpectation(entryId, Marker.Orange, why);

        private static OptionExpectation Unmarked(int entryId, string why) =>
            new OptionExpectation(entryId, Marker.None, why);

        /// <summary>Every option the hub offers, expected to carry nothing.</summary>
        private static OptionExpectation[] AllUnmarked(string why) => new[]
        {
            Unmarked(BuySneakersEntry, why),
            Unmarked(InspectSneakersEntry, why),
            Unmarked(InspectSpeakersEntry, why),
            Unmarked(LeaveEntry, why),
        };
    }
}
