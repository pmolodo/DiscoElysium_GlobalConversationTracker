// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
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

        /// <summary>The white check in the ceiling fan menu: "Grab the tie."</summary>
        /// <remarks>
        /// The only rolled check the harness can rely on being on screen. The fan opens on
        /// 31/64/126/50 every run since de-yzex, and 9:50 is the 50.
        /// </remarks>
        public const int GrabTheTieEntry = 50;

        /// <summary>Where succeeding at <see cref="GrabTheTieEntry"/> leads.</summary>
        /// <remarks>
        /// ASKED OF THE ENGINE, not read off the index: both outcomes of a check link into
        /// the same group and the guards on the check's own flag decide which children are
        /// live, so this is not something a person can work out by hand. The offline tool
        /// answers it - see <c>--branches-of</c> in src/main.rs - and that is how the
        /// fixture below was built.
        /// </remarks>
        public const int GrabTheTiePassEntry = 82;

        /// <summary>Where failing it leads. A different entry, which is the whole point.</summary>
        public const int GrabTheTieFailEntry = 42;

        /// <summary>
        /// The one line Siileng's stall has to be told to go on from, before its menu.
        /// </summary>
        /// <remarks>
        /// MEASURED, 2026-09-04, and the same for every balance and every save that opens
        /// this conversation: the narration is in front of the menu and does not depend on
        /// what is in the player's pocket or on what has been read. Named once rather than
        /// repeated, so a change in the conversation is one edit and not five - and so
        /// that a scenario which legitimately differs stands out.
        ///
        /// It counts lines that WANT an answer. The conversation puts up two, but the
        /// second is the one the menu appears beside, and a line with a menu behind it
        /// needs nothing.
        /// </remarks>
        private const int SiilengAdvances = 1;

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
            /// <param name="advances">
            /// Lines of narration between opening it and its first menu, measured; null
            /// where it has not been.
            /// </param>
            public Somewhere(string save, int conversation, string what, int? advances = null)
            {
                Save = save;
                Conversation = conversation;
                What = what;
                Advances = advances;
            }

            /// <summary>The save that stands in the right place.</summary>
            public string Save { get; }

            /// <summary>The conversation to open from there.</summary>
            public int Conversation { get; }

            /// <summary>Where it is and who is there.</summary>
            public string What { get; }

            /// <summary>
            /// How many lines of narration stand between opening the conversation and its
            /// first menu, or null where nobody has measured it.
            /// </summary>
            /// <remarks>
            /// A property of the place, not of the run: a conversation opens on however
            /// much narration its writer put in front of it, and the harness answers one
            /// line with one continue. Measured by running - the report names the count
            /// it took - and checked from then on, because a conversation that suddenly
            /// needs a different number is not the one the scenario was written against.
            /// </remarks>
            public int? Advances { get; }
        }

        /// <summary>Siileng's stall on the canal, where the sneakers are.</summary>
        public static Somewhere Siileng { get; } =
            new Somewhere("afford-both", 451, "Siileng's stall, on the canal");

        /// <summary>The player's own room, where a new game starts.</summary>
        public static Somewhere CeilingFan { get; } =
            new Somewhere("at-the-fan", 9, "the ceiling fan, in the player's own room", 0);

        /// <summary>Klaasje's room on the Whirling's second floor.</summary>
        public static Somewhere KlaasjesNote { get; } =
            new Somewhere("at-klaasjes-note", 717, "Klaasje's note, in her room", 1);

        /// <summary>The cafeteria on the Whirling's ground floor.</summary>
        public static Somewhere Garte { get; } =
            new Somewhere("at-garte", 28, "Garte, behind the cafeteria counter", 1);

        /// <summary>The balcony off the same floor.</summary>
        public static Somewhere Smoker { get; } =
            new Somewhere("at-the-smoker", 892, "the smoker on the balcony", 0);

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
        /// The ceiling fan's check, and where PASSING it lands - and nothing else.
        /// </summary>
        /// <remarks>
        /// <para>TWO ENTRIES, EACH RECORDED FOR ITS OWN REASON, and between them they put
        /// the two outcomes of one check on two different rungs - which is the one thing
        /// no other fixture arranges.</para>
        ///
        /// <para>9:82 is where the roll SUCCEEDING leads, so recording it makes the word
        /// "Pass" red - a line read in some other save. 9:42, where FAILING leads, is
        /// deliberately absent, so "Fail" stays orange. Both halves are drawn from the same
        /// answer for the same option, so if the two were wired to one outcome they could
        /// not differ, however the fixture was arranged.</para>
        ///
        /// <para>9:50 is the check itself, and it is here to make the search RUN. The
        /// engine refuses a crawl when nothing reachable can outrank the option, so a check
        /// that is itself unseen anywhere - the top rung - is answered without a search and
        /// neither half can carry an asterisk. Recording it drops it a rung and leaves the
        /// headroom the crawl needs.</para>
        /// </remarks>
        private const string FanBranchState = "global-state-fan-pass-recorded.json";

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

        /// <summary>Every suite that exists, in the order a full run would do them.</summary>
        /// <remarks>
        /// <para>Computed rather than stored: a static field would be initialised before
        /// the suites it names, and would quietly hold nulls.</para>
        ///
        /// <para>Everything declared, including what <see cref="Default"/> leaves out, so
        /// that naming a suite explicitly always works and so that the checks which walk
        /// every suite - that its global state exists and that the mod can read it - keep
        /// covering all of them.</para>
        /// </remarks>
        public static IReadOnlyList<LookAheadSuite> All =>
            new[]
            {
                Money, SeenElsewhere, SeenHere, Pristine, BranchOutcomes, BranchGivesUp,
                Budget, SwitchedOff, AllSeen,
            };

        /// <summary>The suites a run does when it is not told which to do.</summary>
        /// <remarks>
        /// <para><see cref="AllSeen"/> is deliberately not here. Its claim - that nothing
        /// is worth crawling once everything is recorded - is about the crawl algorithm
        /// rather than about the game, and it is checked without a game by
        /// <c>AllSeenOfflineTests</c>, over the same state and the same conversations, in
        /// about four seconds and under <c>dotnet test</c>. Repeating it here would cost
        /// a launch and five save loads to learn the same thing.</para>
        ///
        /// <para>It stays available as <c>--suite all-seen</c>, and is worth running that
        /// way when the plumbing rather than the algorithm is in question, since the
        /// offline check cannot see whether the patch is wired up at all.</para>
        /// </remarks>
        public static IReadOnlyList<LookAheadSuite> Default =>
            new[]
            {
                Money, SeenElsewhere, SeenHere, Pristine, BranchOutcomes, BranchGivesUp,
                Budget, SwitchedOff,
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
                    money: 5100,
                    advances: SiilengAdvances,
                    // Nothing in this conversation rolls anything, so nothing in it may
                    // carry a Pass / Fail line - including the option that leads to a
                    // purchase, which is a choice with two outcomes in every sense except
                    // the one the line is about.
                    branchPolicy: BranchPolicy.NoneAnywhere),
                new LookAheadScenario(
                    "afford-only-sneakers",
                    SiilengConversation,
                    "25 centimes left after the sneakers, so the speakers are not affordable",
                    AllUnmarked("the speakers are out of reach once the sneakers are paid for"),
                    money: 5025,
                    advances: SiilengAdvances),
                new LookAheadScenario(
                    "afford-neither",
                    SiilengConversation,
                    "the sneakers cannot be bought at all",
                    AllUnmarked("nothing on the path is affordable"),
                    money: 4900,
                    advances: SiilengAdvances),
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
                    new[]
                    {
                        // The three the money suite marks at this balance. With a budget of
                        // one the crawl cannot reach any of them, and de-pvq is that this
                        // must read as "did not finish" rather than as "nothing there" -
                        // the two used to draw identically, which told the player the
                        // stronger of the two things on the strength of neither.
                        Uncertain(BuySneakersEntry, "the crawl gave up before it could look"),
                        Uncertain(InspectSneakersEntry, "and before it could look here"),
                        Uncertain(InspectSpeakersEntry, "and here"),
                        // LEAVING IS NOT UNCERTAIN, and that is the point rather than an
                        // oversight. It used to be: when the marker came from the managed
                        // engine, a budget of one stopped the crawl before it could
                        // establish anything about any option, this one included.
                        //
                        // The engine refuses a search it can prove cannot find anything -
                        // reaches_potential_improvement, applied at the bridge - and that
                        // refusal builds no state, so no budget can cut it short. Nothing
                        // this option reaches outranks it, which is established here as
                        // firmly at a budget of one as at two hundred thousand. So it draws
                        // plain, which is what "there is nothing down there" looks like.
                        Unmarked(LeaveEntry, "nothing it reaches outranks it, and no budget "
                            + "is needed to know that"),
                    },
                    money: 5100,
                    advances: SiilengAdvances),
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
                    money: 5100,
                    advances: SiilengAdvances,
                    branchPolicy: BranchPolicy.NoneAnywhere),
                // A SECOND CONVERSATION, and one that can offer a rolled check, because
                // the switch has a second thing to turn off now: the Pass / Fail line.
                // Siileng's stall rolls nothing, so on its own it could not tell a switch
                // that works from a line that was never going to be drawn there.
                new LookAheadScenario(
                    Smoker.Save,
                    Smoker.Conversation,
                    $"{Smoker.What}, which can offer a check, with the feature switched off",
                    Array.Empty<OptionExpectation>(),
                    markers: MarkerPolicy.NoneAnywhere,
                    advances: Smoker.Advances,
                    branchPolicy: BranchPolicy.NoneAnywhere),
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
                    money: 5100,
                    advances: SiilengAdvances),
            });

        /// <summary>
        /// A conversation this save has read to the end earns nothing.
        /// </summary>
        /// <remarks>
        /// The bottom rung. Every entry is read in this save, so every option's own
        /// novelty and everything it can reach are both the lowest, and nothing can
        /// outrank anything. The global state and local save are both exhaustive, so the
        /// structural scan can prove that before any crawl state is built. Statistics
        /// distinguish that shortcut from crawls that ran and found nothing.
        /// </remarks>
        public static LookAheadSuite SeenHere { get; } = new LookAheadSuite(
            "seen-here",
            "a conversation read to the end earns no marker and needs no crawl",
            AllSeenElsewhereState,
            new[]
            {
                new LookAheadScenario(
                    "seen-here-all",
                    SiilengConversation,
                    "every entry read in this save",
                    AllUnmarked("there is nothing here this save has not read"),
                    money: 5100,
                    advances: SiilengAdvances),
            },
            pluginSettings: KeepStatistics,
            artefacts: new[]
            {
                new SuiteArtefact(
                    "look-ahead-stats.json",
                    "no crawl ran because every scoreable entry is already seen here",
                    NoCrawls),
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
        /// <remarks>
        /// It says something about the Pass / Fail lines all the same, and can, because
        /// the claim is a rule rather than a list: on a profile that has recorded nothing,
        /// every outcome of every check lands on text no save has read, so both words are
        /// orange - and nothing can outrank the top rung, so neither carries an asterisk.
        /// Every option that rolls nothing gets no line at all, which is the half that
        /// catches a line invented for an option with one outcome.
        ///
        /// A FAILURE HERE IS WORTH READING BEFORE IT IS BELIEVED. These saves are real
        /// playthroughs, so an entry may be read in the SAVE while the global state is
        /// empty; a check whose outcome lands on one of those would draw that word dark
        /// red, correctly. The run prints every line it read, so the log says which.
        /// </remarks>
        private static LookAheadScenario Nothing(Somewhere where) =>
            new LookAheadScenario(
                where.Save,
                where.Conversation,
                $"{where.What}, on a profile that has recorded nothing",
                Array.Empty<OptionExpectation>(),
                markers: MarkerPolicy.NoneAnywhere,
                advances: where.Advances,
                branchPolicy: BranchPolicy.EveryCheck,
                branches: new BranchExpectation(
                    new BranchHalf(BranchColour.Orange),
                    new BranchHalf(BranchColour.Orange),
                    "nothing has been read anywhere, so both outcomes land on unread text "
                        + "and neither can reach anything that outranks it"));

    /// <summary>
    /// Checks that nothing was actually searched.
    /// </summary>
    /// <remarks>
    /// <para>The statistics are written at shutdown only when a crawl was recorded, so on
    /// a pristine profile the file is legitimately absent. A file that IS there must
    /// report that no state was ever built.</para>
    ///
    /// <para>STATES RATHER THAN CRAWLS, since the marker moved to the bridge (de-i5xj.6).
    /// The shortcut that makes this suite's claim true now fires inside the engine, past
    /// the point where the plugin has counted an ask - so the ask is recorded either way
    /// and counting asks would fail a suite that is behaving perfectly. What the shortcut
    /// still shows, and what this suite is really about, is that the search cost nothing:
    /// zero states explored, over however many options were asked about.</para>
    /// </remarks>
    private static string? NoCrawls(string? json)
    {
        if (json is null)
        {
            return null;
        }

        using JsonDocument document = JsonDocument.Parse(json);
        long states = document.RootElement
            .GetProperty("states").GetProperty("total").GetInt64();
        if (states == 0)
        {
            return null;
        }

        long crawls = document.RootElement.GetProperty("crawls").GetInt64();
        return $"{states} states were explored over {crawls} crawls, but none should have "
            + "been - nothing here outranks any option, so every search should have been "
            + "refused before it built a state";
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
        /// The biggest conversations in the game, with everything already recorded, cost
        /// nothing at all.
        /// </summary>
        /// <remarks>
        /// <para>This suite used to be called "headroom" and used to be a measurement:
        /// the staged global state records every entry of every conversation, which was
        /// once the most expensive shape a crawl could take, because no option's own
        /// novelty is unseen-anywhere - so the early exit in MarkerFor did not fire - and
        /// nothing reachable is unseen-anywhere either, so a crawl could not stop the
        /// instant it found something and had to explore everything.</para>
        ///
        /// <para>The no-potential-improvement short-circuit ended that. When every entry
        /// is recorded, nothing can outrank the option that is being asked about, so no
        /// walk can produce a marker and the crawl is skipped before any state is built.
        /// The expensive shape is now the near-opposite - a group with one unseen node,
        /// or a handful - and that is measured elsewhere.</para>
        ///
        /// <para>So what these five conversations are for now is the strongest available
        /// statement of the cheap case. They are the largest in the game, so if a crawl
        /// were going to run anywhere it would run here, and the claim is that not one
        /// does. That is why the biggest conversations are still the right scenarios for
        /// it even though nothing is being timed.</para>
        /// </remarks>
        public static LookAheadSuite AllSeen { get; } = new LookAheadSuite(
            "all-seen",
            "the biggest conversations cost nothing when every entry is already recorded",
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
                // Kept on so that a crawl WOULD leave a trace. The claim is that the file
                // is absent; that means nothing unless the run was configured to write it.
                ["KeepLookAheadStates"] = "true",
                ["LogLookAheadBudgetExceeded"] = "true",
            },
            artefacts: new[]
            {
                new SuiteArtefact(
                    "look-ahead-stats.json",
                    "no crawl ran, because nothing here can outrank any option",
                    NoCrawls),
                new SuiteArtefact(
                    "look-ahead-budget-overflows.log",
                    "and so no crawl spent a budget either",
                    ReportOverflows),
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
    ///
    /// <para>NO SUITE WIRES THIS UP AT PRESENT, and that is deliberate rather than an
    /// oversight. The one suite that measured a cost - the old "headroom" - now claims a
    /// no-crawl instead, because the state it stages makes every crawl skippable. This
    /// is kept because the shapes that ARE expensive under the short-circuit, a group
    /// with one unseen node or a handful, still need exactly this table, and because
    /// asking for a cost on any suite is worth having as an option rather than as a
    /// property of one suite. Do not delete it as unused.</para>
    /// </remarks>
    public static string? ReportCost(string? json)
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

    /// <summary>
    /// Prints the overflow report, which says WHERE a crawl's states went.
    /// </summary>
    /// <remarks>
    /// <para>The cost table says a crawl spent its whole budget; this says what it spent
    /// it on. Each overflow names the option, the state it started from, how many slots
    /// the group interned, and the entries reached in the most distinct states - and it
    /// is that last list that identifies a blow-up, because a conversation whose states
    /// are spread evenly over a thousand entries is a different problem from one where a
    /// single hub accounts for most of them.</para>
    ///
    /// <para>The mod writes this only for a crawl that actually overflowed, and only
    /// when asked, because producing it means walking the offending option a second time
    /// with a per-entry tally. Nothing here asserts on it: an overflow is expected in
    /// this suite, and the report is evidence rather than a claim. It is read at all
    /// because the profile it is written into is staged, and goes away with it.</para>
    /// </remarks>
    private static string? ReportOverflows(string? text)
    {
        if (text is null)
        {
            // Not a failure. It means nothing overflowed, which the cost table already
            // reports, and which would be good news.
            return null;
        }

        Console.WriteLine();
        foreach (string line in text.Split('\n'))
        {
            Console.WriteLine($"        {line.TrimEnd()}");
        }

        return null;
    }

        /// <summary>Finds the requested suites.</summary>
        /// <param name="names">
        /// Suite names, or an empty list for <see cref="Default"/>. Naming a suite finds
        /// it in <see cref="All"/>, so one left out of the default run is still asked for
        /// by name.
        /// </param>
        /// <exception cref="ArgumentException">No suite goes by a requested name.</exception>
        public static IReadOnlyList<LookAheadSuite> SelectMany(IReadOnlyList<string> names)
        {
            if (names == null)
            {
                throw new ArgumentNullException(nameof(names));
            }

            if (names.Count == 0)
            {
                return Default;
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

        /// <summary>How a scenario's save is separated from its conversation in a name.</summary>
        private const char ScenarioSeparator = ':';

        /// <summary>
        /// The same suites over only the scenarios named, or all of them when none is.
        /// </summary>
        /// <remarks>
        /// <para>WHY A RUN WANTS THIS. A suite is a launch and its scenarios are save loads
        /// inside it, and the big suites are five or six - so asking about one menu costs
        /// the other five every time, at half a minute each, with the display taken over
        /// for all of it. Iterating on one expectation is exactly when that is least
        /// affordable and exactly when it happens most.</para>
        ///
        /// <para>A NAME IS A SAVE, OR A SAVE AND A CONVERSATION. Neither alone identifies a
        /// scenario: the pristine suite opens four conversations from one save, and
        /// at-the-fan is used by three suites. So "at-the-fan" takes every scenario that
        /// loads it and "at-the-fan:9" takes the one that opens conversation 9 from it.</para>
        ///
        /// <para>A NAME THAT MATCHES NOTHING IS AN ERROR rather than an empty run. The
        /// whole purpose of the flag is to run less, so a typo that ran nothing at all and
        /// reported it as a pass would be the worst thing it could do.</para>
        /// </remarks>
        /// <param name="suites">The suites to filter, already selected by name.</param>
        /// <param name="scenarioNames">Save names or save:conversation pairs.</param>
        /// <returns>The suites that keep at least one scenario, in their original order.</returns>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="ArgumentException">A name matches no scenario.</exception>
        public static IReadOnlyList<LookAheadSuite> Only(
            IReadOnlyList<LookAheadSuite> suites, IReadOnlyList<string> scenarioNames)
        {
            if (suites == null)
            {
                throw new ArgumentNullException(nameof(suites));
            }

            if (scenarioNames == null)
            {
                throw new ArgumentNullException(nameof(scenarioNames));
            }

            if (scenarioNames.Count == 0)
            {
                return suites;
            }

            foreach (string name in scenarioNames)
            {
                if (!suites.SelectMany(suite => suite.Scenarios).Any(s => Names(s, name)))
                {
                    throw new ArgumentException(
                        $"No scenario called '{name}' in the selected suite(s). Available: "
                        + string.Join(", ", Available(suites)) + ".",
                        nameof(scenarioNames));
                }
            }

            return suites
                .Select(suite => suite.WithScenarios(
                    suite.Scenarios
                        .Where(s => scenarioNames.Any(name => Names(s, name)))
                        .ToArray()))
                .Where(suite => suite.Scenarios.Count > 0)
                .ToArray();
        }

        /// <summary>Whether one name picks out one scenario.</summary>
        private static bool Names(LookAheadScenario scenario, string name)
        {
            int separator = name.IndexOf(ScenarioSeparator);
            if (separator < 0)
            {
                return string.Equals(
                    scenario.SaveName, name, StringComparison.OrdinalIgnoreCase);
            }

            return string.Equals(
                    scenario.SaveName,
                    name.Substring(0, separator),
                    StringComparison.OrdinalIgnoreCase)
                && int.TryParse(
                    name.Substring(separator + 1),
                    NumberStyles.Integer,
                    CultureInfo.InvariantCulture,
                    out int conversation)
                && conversation == scenario.ConversationId;
        }

        /// <summary>Every scenario of these suites, named as the filter wants them.</summary>
        private static IEnumerable<string> Available(IReadOnlyList<LookAheadSuite> suites) =>
            suites
                .SelectMany(suite => suite.Scenarios)
                .Select(s => s.SaveName + ScenarioSeparator + s.ConversationId)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .OrderBy(name => name, StringComparer.OrdinalIgnoreCase);

        /// <summary>Finds one suite by name, or every suite when no name is given.</summary>
        /// <param name="name">The suite's name, or null for the default run.</param>
        /// <returns>The matching suite, or <see cref="Default"/>.</returns>
        /// <exception cref="ArgumentException">No suite goes by that name.</exception>
        public static IReadOnlyList<LookAheadSuite> Select(string? name)
        {
            return name == null
                ? Default
                : SelectMany(new[] { name });
        }

        /// <summary>The ceiling fan, with one outcome of its check recorded elsewhere.</summary>
        /// <remarks>
        /// <para>THE CLAIM THE FEATURE EXISTS FOR, and the one every other suite stops
        /// short of: Pass saying one thing while Fail says another, on one option, in the
        /// running game. The pristine suite has both halves orange, the switched-off suite
        /// has no line at all, and neither could tell a line whose two halves are wired to
        /// the two outcomes from one that draws the same answer twice.</para>
        ///
        /// <para>Everything the fixture arranges is described on
        /// <see cref="FanBranchState"/>. What it produces, which is what is asserted here:
        /// Pass red because 9:82 is recorded, with an orange asterisk because the pass
        /// branch runs on into entries nothing has recorded; Fail orange, on the top rung,
        /// where nothing can outrank it and no asterisk is possible. Both the colour and
        /// the asterisk differ, so a line built from one outcome twice fails this twice
        /// over.</para>
        ///
        /// <para>PREDICTED BEFORE IT WAS RUN. The offline tool was asked what the mod
        /// would draw from this exact fixture - the same bridge call, over the shipped
        /// index - and the in-game run then agreed with it. That is worth knowing because
        /// it is how the next fixture of this kind should be built: arranging a state by
        /// hand and running the game to see what happens costs minutes per guess.</para>
        /// </remarks>
        public static LookAheadSuite BranchOutcomes { get; } = new LookAheadSuite(
            "branch-outcomes",
            "a check's two outcomes are drawn from their own answers, and can differ",
            FanBranchState,
            new[]
            {
                new LookAheadScenario(
                    CeilingFan.Save,
                    CeilingFan.Conversation,
                    "the ceiling fan, with the check's PASS outcome recorded elsewhere and "
                        + "its FAIL outcome recorded nowhere",
                    new[]
                    {
                        Unmarked(
                            GrabTheTieEntry,
                            "a rolled check has a line below it saying what each outcome "
                                + "reaches, so it keeps no marker of its own - and there "
                                + "is one to keep here, since the unread text down the "
                                + "pass branch outranks the recorded check"),
                    },
                    advances: CeilingFan.Advances,
                    branchPolicy: BranchPolicy.EveryCheck,
                    branches: new BranchExpectation(
                        new BranchHalf(BranchColour.Red, Marker.Orange),
                        new BranchHalf(BranchColour.Orange),
                        $"passing lands on {CeilingFan.Conversation}:{GrabTheTiePassEntry}, "
                            + "which is recorded, and can still reach text that is not; "
                            + $"failing lands on {CeilingFan.Conversation}:"
                            + $"{GrabTheTieFailEntry}, which nothing has recorded and which "
                            + "nothing can outrank")),
            },
            pluginSettings: KeepStatistics);

        /// <summary>The same check, with a budget too small to answer either outcome.</summary>
        /// <remarks>
        /// <para>de-pvq's grey '*?' ON A HALF OF THE LINE rather than on an option. The
        /// budget suite covers the uncertain marker, but it runs at Siileng's stall, whose
        /// menu holds no rolled check - so until this suite nothing in game had ever seen a
        /// Pass or Fail word give up.</para>
        ///
        /// <para>The distinction being protected is the same one: "the search did not
        /// finish" and "the search found nothing" are different answers, and a half that
        /// drew them alike would tell the player the stronger of the two on the strength of
        /// neither. Here the previous suite's run is the control - the same fixture, the
        /// same menu, and the only difference is the budget - so a grey half can only be
        /// the budget.</para>
        ///
        /// <para>THE SAME BUDGET THE BUDGET SUITE USES, and for the same reason: a state
        /// count is the only limit that gives up at exactly the same point on every
        /// machine. When de-7z0f replaces that setting, this suite and that one need the
        /// replacement together.</para>
        /// </remarks>
        public static LookAheadSuite BranchGivesUp { get; } = new LookAheadSuite(
            "branch-gives-up",
            "each half of a check's line can say it gave up, rather than saying nothing",
            FanBranchState,
            new[]
            {
                new LookAheadScenario(
                    CeilingFan.Save,
                    CeilingFan.Conversation,
                    "the same check, with a budget of one",
                    new[]
                    {
                        Unmarked(
                            GrabTheTieEntry,
                            "the crawl gave up, and a rolled check draws no marker of its "
                                + "own either way - each half of its line says whether "
                                + "that outcome's search finished"),
                    },
                    advances: CeilingFan.Advances,
                    branchPolicy: BranchPolicy.EveryCheck,
                    branches: new BranchExpectation(
                        new BranchHalf(BranchColour.Red, Marker.Uncertain),
                        new BranchHalf(BranchColour.Orange),
                        "where each outcome LANDS is read off the graph and costs no "
                            + "search, so both words keep their colours; the pass half is "
                            + "what the budget stopped, so its asterisk is grey - and the "
                            + "fail half lands on the top rung, where no search is run at "
                            + "all and nothing could have outranked it if one had")),
            },
            pluginSettings: new Dictionary<string, string>
            {
                ["LookAheadStateBudget"] = "1",
            });

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
        /// <summary>An option whose crawl is expected to give up before it can answer.</summary>
        private static OptionExpectation Uncertain(int entryId, string why) =>
            new OptionExpectation(entryId, Marker.Uncertain, why);

        private static OptionExpectation[] AllUnmarked(string why) => new[]
        {
            Unmarked(BuySneakersEntry, why),
            Unmarked(InspectSneakersEntry, why),
            Unmarked(InspectSpeakersEntry, why),
            Unmarked(LeaveEntry, why),
        };
    }
}
