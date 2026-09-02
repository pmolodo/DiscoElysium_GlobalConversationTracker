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
        /// <summary>
        /// The look-ahead time budget every suite runs with, in milliseconds.
        /// </summary>
        /// <remarks>
        /// Far above anything a crawl here takes - the worst measured over the largest
        /// conversations in the game is about three quarters of a second - so the clock
        /// never fires and the state budget stays the only limit that decides anything.
        /// That is the point. A wall-clock limit is not reproducible: the same menu on
        /// the same save could mark differently on a machine that happened to be busy,
        /// and a suite that can flip on load is worse than no suite. The shipped default
        /// is a second, and testing that would be testing the clock.
        /// </remarks>
        public const int TestTimeBudgetMs = 30_000;

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

        /// <summary>
        /// A conversation, and a save that puts the player beside the actor it belongs to.
        /// </summary>
        /// <remarks>
        /// The pairing is the point. The probe will open any conversation by id from
        /// anywhere, which is convenient and wrong: a conversation started somewhere else
        /// can stall outright - its sequences reach for a scene that is not loaded, and it
        /// sits there active with no menu - and where it does run it may take branches it
        /// would not take in its own place. Every position here is one the game itself
        /// wrote for a character in a real save, chosen as the nearest recorded position
        /// to that actor, because a recorded position is known to be standing and on the
        /// navmesh.
        /// </remarks>
        public sealed class Somewhere
        {
            /// <summary>Creates a pairing.</summary>
            /// <param name="save">The save that stands in the right place.</param>
            /// <param name="conversation">The conversation to open from there.</param>
            /// <param name="what">Where it is and who is there.</param>
            public Somewhere(string save, int conversation, string what)
            {
                Save = save;
                Conversation = conversation;
                What = what;
            }

            /// <summary>The save that stands in the right place.</summary>
            public string Save { get; }

            /// <summary>The conversation to open from there.</summary>
            public int Conversation { get; }

            /// <summary>Where it is and who is there.</summary>
            public string What { get; }
        }

        /// <summary>Siileng's stall on the canal, where the sneakers are.</summary>
        public static Somewhere Siileng { get; } =
            new Somewhere("afford-both", 451, "Siileng's stall, on the canal");

        /// <summary>The player's own room, where a new game starts.</summary>
        public static Somewhere CeilingFan { get; } =
            new Somewhere("at-the-fan", 9, "the ceiling fan, in the player's own room");

        /// <summary>Klaasje's room on the Whirling's second floor.</summary>
        public static Somewhere KlaasjesNote { get; } =
            new Somewhere("at-klaasjes-note", 717, "Klaasje's note, in her room");

        /// <summary>The cafeteria on the Whirling's ground floor.</summary>
        public static Somewhere Garte { get; } =
            new Somewhere("at-garte", 28, "Garte, behind the cafeteria counter");

        /// <summary>The balcony off the same floor.</summary>
        public static Somewhere Smoker { get; } =
            new Somewhere("at-the-smoker", 892, "the smoker on the balcony");

        /// <summary>Joyce's sloop, at the pier.</summary>
        public static Somewhere Joyce { get; } =
            new Somewhere("at-joyce", 631, "Joyce, on her sloop at the pier");

        /// <summary>The tree in the yard behind the Whirling.</summary>
        public static Somewhere HangedMan { get; } =
            new Somewhere("at-the-hanged-man", 14, "the hanged man, in the yard");

        /// <summary>The fishing village, far along the coast.</summary>
        public static Somewhere DoomSpiral { get; } =
            new Somewhere("at-the-doom-spiral", 1030, "the doom spiral, in the village");

        /// <summary>The students' flat, up its own staircase off the street.</summary>
        public static Somewhere Steban { get; } =
            new Somewhere("at-steban", 362, "Steban, in the students' flat");

        /// <summary>The wall the young communists have made their own.</summary>
        public static Somewhere Noid { get; } =
            new Somewhere("at-noid", 368, "Noid, under the mural");

        /// <summary>The global state all three money scenarios share.</summary>
        /// <remarks>
        /// Every entry of the conversation except 80, so reaching 80 is the only way to
        /// find something no save has read.
        /// </remarks>
        private const string MoneyState = "global-conversation-state.json";

        /// <summary>
        /// Every entry recorded, so nothing the crawl reaches is unseen anywhere.
        /// </summary>
        private const string AllSeenElsewhereState = "global-state-all-seen-elsewhere.json";

        /// <summary>Nothing recorded, so every option is itself unseen anywhere.</summary>
        private const string EmptyState = "global-state-empty.json";

        /// <summary>
        /// Every entry of the biggest conversations recorded, which is the most
        /// expensive shape a crawl can have.
        /// </summary>
        private const string WorstCaseState = "global-state-worst-case.json";

        /// <summary>
        /// The largest conversations that can be reached from a place the player can
        /// stand, by entry count.
        /// </summary>
        /// <remarks>
        /// Entry count is not the cost - the budget counts (entry, state) pairs, so what
        /// blows up is the state slots a conversation touches multiplied by its reachable
        /// entries - and it turns out to be a poor proxy: 1030 is the sixth largest
        /// conversation in the game and its crawls peak at 201 states, a thousandth of
        /// Joyce's. It is still the only ordering available without running them, and it
        /// is what these six are: the six largest of the game's 1,501 conversations.
        /// </remarks>
        public static IReadOnlyList<Somewhere> BiggestConversations =>
            new[] { Noid, HangedMan, Joyce, Garte, DoomSpiral };

        /// <summary>Every suite, in the order a full run does them.</summary>
        /// <remarks>
        /// Computed rather than stored: a static field would be initialised before the
        /// suites it names, and would quietly hold nulls.
        /// </remarks>
        public static IReadOnlyList<LookAheadSuite> All =>
            new[]
            {
                Money, SeenElsewhere, SeenHere, Pristine, Budget, SwitchedOff, Headroom,
            };

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
            // Costs this suite nothing - it makes the mod write a summary it would
            // otherwise keep to itself - and this is the suite with the most crawls to
            // summarise, so it is the cheapest place to check the summary is right
            // rather than paying for another launch.
            pluginSettings: KeepStatistics,
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
    private static string? CheckStatistics(string? json)
    {
        if (json is null)
        {
            return "look-ahead-stats.json was never written";
        }

        return CheckStatisticsOf(json);
    }

    private static string? CheckStatisticsOf(string json)
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
                    CheckOverflowLog),
            });

        /// <summary>
        /// Turning the feature off leaves the options alone and the tracking working.
        /// </summary>
        /// <remarks>
        /// <para>The setting is documented as leaving tracking unaffected, and nothing
        /// checked either half of that.</para>
        ///
        /// <para>Unmarked options are paired with the harness's suite-prepared
        /// acknowledgement, which reports the runtime setting it applied. The tracking
        /// hook line remains an independent check that tracking stayed active.</para>
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
                    AllUnmarked("look-ahead marking is disabled"),
                    money: 5100),
            },
            pluginSettings: new Dictionary<string, string>
            {
                ["MarkLookAhead"] = "false",
            },
            logExpectations: new[]
            {
                new LogExpectation(
                    "dialogue statuses are being tracked",
                    true,
                    "tracking is unaffected by the switch"),
            });

        /// <summary>
        /// Reaching a line another save has read, from an option this one has, is red.
        /// </summary>
        /// <remarks>
        /// <para>The rung that has never run in game. It needs both halves of the ladder
        /// at once: the option's own entry read in THIS save, so its own novelty is the
        /// lowest rung, and everything the crawl reaches recorded in the global state but
        /// not in the save, so the best it can find is the middle one.</para>
        ///
        /// <para>Entries 33 and 67 are read in the save and so should be marked; 85 is
        /// not read and reaches nothing anyway; 86 is not read either, so its own novelty
        /// already equals the best thing it can reach and the rule says leave it alone.
        /// That last one is what makes this more than a colour check - it is the ordering
        /// rule failing to fire, in the same menu as it fires twice.</para>
        /// </remarks>
        public static LookAheadSuite SeenElsewhere { get; } = new LookAheadSuite(
            "seen-elsewhere",
            "an option this save has read, leading somewhere only another save has, is red",
            AllSeenElsewhereState,
            new[]
            {
                new LookAheadScenario(
                    "seen-here-some",
                    SiilengConversation,
                    "two options read in this save, everything recorded in another",
                    new[]
                    {
                        Marked(InspectSneakersEntry, Marker.Red,
                            "read here, and it leads on to lines only another save has read"),
                        Marked(InspectSpeakersEntry, Marker.Red, "and so does the other"),
                        Unmarked(BuySneakersEntry,
                            "not read here, so it already ranks as high as anything it reaches"),
                        Unmarked(LeaveEntry, "leaving reaches nothing at all"),
                    },
                    money: 5100),
            });

        /// <summary>
        /// A conversation this save has read to the end earns nothing.
        /// </summary>
        /// <remarks>
        /// The bottom rung. Every entry is read in this save, so every option's own
        /// novelty and everything it can reach are both the lowest, and nothing can
        /// outrank anything. Distinguished from a crawl that simply did not run by the
        /// statistics, which must still record crawls.
        /// </remarks>
        public static LookAheadSuite SeenHere { get; } = new LookAheadSuite(
            "seen-here",
            "a conversation already read to the end earns no marker",
            AllSeenElsewhereState,
            new[]
            {
                new LookAheadScenario(
                    "seen-here-all",
                    SiilengConversation,
                    "every entry read in this save",
                    AllUnmarked("there is nothing here this save has not read"),
                    money: 5100),
            },
            pluginSettings: KeepStatistics,
            artefacts: new[]
            {
                new SuiteArtefact(
                    "look-ahead-stats.json",
                    "the crawls ran and found nothing, rather than not running",
                    CheckStatistics),
            });

        /// <summary>
        /// An option that is itself unread anywhere is never marked, and never crawled.
        /// </summary>
        /// <remarks>
        /// <para>The rule that gives the feature its shape: nothing outranks where such
        /// an option already leads, so a marker would say nothing. On a profile that has
        /// recorded nothing at all - a first playthrough - no option in the game is ever
        /// marked, which is a strong claim and worth holding to.</para>
        ///
        /// <para>And it is decided WITHOUT crawling. MarkerFor answers from the option's
        /// own novelty and returns before it builds a graph, so a first playthrough pays
        /// nothing at all for the feature. That is why this suite asserts the statistics
        /// record no crawl rather than crawls that found nothing - the difference between
        /// the two is the whole of the optimisation.</para>
        /// </remarks>
        public static LookAheadSuite Pristine { get; } = new LookAheadSuite(
            "pristine",
            "an option that is itself unread anywhere is never marked, and never crawled",
            EmptyState,
            PristineScenarios,
            pluginSettings: KeepStatistics,
            artefacts: new[]
            {
                new SuiteArtefact(
                    "look-ahead-stats.json",
                    "no crawl ran at all, because none could have said anything",
                    NoCrawls),
            });

        /// <summary>
        /// The same fresh profile, put to several conversations of different shapes.
        /// </summary>
        /// <remarks>
        /// The claim is about the whole game rather than one stall, so it is worth asking
        /// it of more than one conversation - and this is the suite that can, since it
        /// needs no global state and no save of its own. The probe opens any conversation
        /// by id wherever the player is standing, so five cost one launch and about five
        /// seconds each.
        ///
        /// Named for their shapes rather than at random: the ceiling fan has no purchase
        /// in it, Klaasje's note is small and heavily gated, the smoker is mid-sized, and
        /// Garte is one of the largest conversations in the game.
        /// </remarks>
        private static LookAheadScenario[] PristineScenarios =>
            new[] { CeilingFan, KlaasjesNote, Smoker, Garte }
                .Select(Nothing)
                .ToArray();

        /// <summary>A scenario that says nothing in the menu should be marked.</summary>
        private static LookAheadScenario Nothing(Somewhere where) =>
            new LookAheadScenario(
                where.Save,
                where.Conversation,
                $"{where.What}, on a profile that has recorded nothing",
                Array.Empty<OptionExpectation>(),
                markers: MarkerPolicy.NoneAnywhere);

    /// <summary>
    /// Checks that nothing was crawled.
    /// </summary>
    /// <remarks>
    /// The statistics are written at shutdown only when a crawl was recorded, so on a
    /// pristine profile the file is legitimately absent. A file that IS there must
    /// report no crawl.
    /// </remarks>
    private static string? NoCrawls(string? json)
    {
        if (json is null)
        {
            return null;
        }

        using JsonDocument document = JsonDocument.Parse(json);
        int crawls = document.RootElement.GetProperty("crawls").GetInt32();
        return crawls == 0 ? null : $"{crawls} crawls ran, but none should have";
    }

    /// <summary>Checks the overflow log names the conversation that ran out.</summary>
    private static string? CheckOverflowLog(string? text)
    {
        if (text is null)
        {
            return "look-ahead-budget-overflows.log was never written";
        }

        return text.Contains("budget exhausted", StringComparison.Ordinal)
            && text.Contains($"{SiilengConversation}:", StringComparison.Ordinal)
            ? null
            : $"no overflow block for conversation {SiilengConversation} in "
                + $"{text.Length} characters";
    }

        /// <summary>
        /// What the biggest conversations in the game actually cost.
        /// </summary>
        /// <remarks>
        /// <para>Not a behaviour check - a measurement, with assertions about what the
        /// measurement found. The staged global state records every entry of every
        /// conversation here, which is the most expensive shape a crawl can take: no
        /// option's own novelty is unseen-anywhere, so the early exit in MarkerFor does
        /// not fire and the crawl runs; and nothing it reaches is unseen-anywhere either,
        /// so it cannot stop the instant it finds something and has to explore everything
        /// reachable.</para>
        ///
        /// <para>What the run prints alongside is the cost - states and milliseconds,
        /// worst and mean, per conversation - because whether the budget was reached is
        /// one bit, and how close it came is what says whether the limit is doing
        /// anything and whether a larger one would be affordable.</para>
        /// </remarks>
        public static LookAheadSuite Headroom { get; } = new LookAheadSuite(
            "headroom",
            "what the biggest conversations cost, and whether the budget is ever reached",
            WorstCaseState,
            BiggestConversations
                .Select(where => new LookAheadScenario(
                    where.Save,
                    where.Conversation,
                    where.What,
                    Array.Empty<OptionExpectation>(),
                    markers: MarkerPolicy.Ignored))
                .ToArray(),
            pluginSettings: new Dictionary<string, string>
            {
                ["KeepLookAheadStates"] = "true",
                ["LogLookAheadBudgetExceeded"] = "true",
            },
            artefacts: new[]
            {
                new SuiteArtefact(
                    "look-ahead-stats.json",
                    "the crawls ran and are all accounted for, at a cost the run prints",
                    ReportCost),
            });

    /// <summary>
    /// Prints what the crawls cost, and checks the figures account for themselves.
    /// </summary>
    /// <remarks>
    /// <para>The printing is the point; the assertion only guards it. Crawls have to have
    /// happened - a measurement of nothing prints an empty table and would otherwise
    /// pass - and the per-conversation rows have to add up to the total, since a row
    /// missing from the breakdown is cost the table does not show.</para>
    ///
    /// <para>Deliberately NOT keyed to the conversations the scenarios open. The
    /// statistics are recorded against the conversation each OPTION belongs to, and an
    /// option's conversation is often not the one that is open: 28 (WHIRLING F1 / GARTE
    /// MAIN) draws a menu whose four options are all entries of 13 (WHIRLING F1 / GARTE),
    /// so a run that crawls Garte's menu perfectly well records nothing under 28.
    /// Demanding a row per opened conversation fails on correct behaviour.</para>
    /// </remarks>
    private static string? ReportCost(string? json)
    {
        if (json is null)
        {
            return "look-ahead-stats.json was never written, so nothing was crawled";
        }

        using JsonDocument document = JsonDocument.Parse(json);
        JsonElement root = document.RootElement;

        Console.WriteLine();
        Console.WriteLine(
            "        conversation  crawls  max states  max ms  mean states  "
            + "spent  of which time");

        int counted = 0;
        int outOfTime = 0;
        foreach (JsonElement row in root.GetProperty("byConversation").EnumerateArray())
        {
            int crawls = row.GetProperty("crawls").GetInt32();
            int timed = Count(row, "timeExhausted");
            counted += crawls;
            outOfTime += timed;

            Console.WriteLine(
                $"        {row.GetProperty("conversation").GetInt32(),12}  {crawls,6}  "
                + $"{row.GetProperty("maxStates").GetInt32(),10}  "
                + $"{row.GetProperty("maxMs").GetDouble(),6:N1}  "
                + $"{row.GetProperty("meanStates").GetDouble(),11:N1}  "
                + $"{row.GetProperty("budgetExhausted").GetInt32(),5}  {timed,13}");
        }

        int total = root.GetProperty("crawls").GetInt32();
        Console.WriteLine();
        Console.WriteLine(
            $"        overall: {total} crawls, "
            + $"{root.GetProperty("states").GetProperty("max").GetInt32()} states at worst, "
            + $"{root.GetProperty("milliseconds").GetProperty("max").GetDouble():N1} ms at worst");
        Console.WriteLine($"        histogram: {root.GetProperty("statesHistogram")}");
        Console.WriteLine();

        if (total <= 0)
        {
            return "no crawl ran at all, so there is nothing here to measure";
        }

        if (counted != total)
        {
            return $"{total} crawls overall but {counted} in the per-conversation breakdown";
        }

        // A measurement the clock cut short is a measurement of the machine. The suite
        // runs with a time budget far above anything a crawl here takes precisely so this
        // cannot happen; if it did, the states and milliseconds above are lower bounds
        // and the run needs repeating on a quieter machine.
        return outOfTime == 0
            ? null
            : $"{outOfTime} crawl(s) ran out of TIME rather than states, so these figures "
                + "are bounded by the clock rather than by the conversations";
    }

    /// <summary>
    /// Reads a count that a plugin older than the field would not have written.
    /// </summary>
    private static int Count(JsonElement row, string name) =>
        row.TryGetProperty(name, out JsonElement value) ? value.GetInt32() : 0;

        /// <summary>Finds the requested suites.</summary>
        /// <param name="names">Suite names, or an empty list for every suite.</param>
        /// <exception cref="ArgumentException">No suite goes by a requested name.</exception>
        public static IReadOnlyList<LookAheadSuite> SelectMany(IReadOnlyList<string> names)
        {
            if (names == null)
            {
                throw new ArgumentNullException(nameof(names));
            }

            if (names.Count == 0)
            {
                return All;
            }

            var selected = new List<LookAheadSuite>();
            foreach (string name in names)
            {
                LookAheadSuite? found = All.FirstOrDefault(
                    suite => string.Equals(suite.Name, name, StringComparison.OrdinalIgnoreCase));

                if (found == null)
                {
                    throw new ArgumentException(
                        $"No look-ahead suite called '{name}'. Known suites: "
                        + string.Join(", ", All.Select(s => s.Name)) + ".",
                        nameof(names));
                }

                if (!selected.Contains(found))
                {
                    selected.Add(found);
                }
            }

            return selected;
        }

        /// <summary>Finds one suite by name, or every suite when no name is given.</summary>
        /// <param name="name">The suite's name, or null for every suite.</param>
        /// <returns>The matching suite, or every suite.</returns>
        /// <exception cref="ArgumentException">No suite goes by that name.</exception>
        public static IReadOnlyList<LookAheadSuite> Select(string? name)
        {
            return name == null
                ? All
                : SelectMany(new[] { name });
        }

        /// <summary>Asking the mod to keep the statistics a suite reads back.</summary>
        private static Dictionary<string, string> KeepStatistics =>
            new Dictionary<string, string> { ["KeepLookAheadStates"] = "true" };

        private static OptionExpectation Marked(int entryId, Marker marker, string why) =>
            new OptionExpectation(entryId, marker, why);

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
