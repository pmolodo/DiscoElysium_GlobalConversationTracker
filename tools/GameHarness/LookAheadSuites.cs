// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
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
        /// never fires and the memory budget stays the only limit that decides anything.
        /// That is the point. A wall-clock limit is not reproducible: the same menu on
        /// the same save could mark differently on a machine that happened to be busy,
        /// and a suite that can flip on load is worse than no suite. The shipped default
        /// is a second, and testing that would be testing the clock.
        /// </remarks>
        public const int TestTimeBudgetMs = 30_000;

        /// <summary>
        /// The whole-menu time budget every suite runs with, in milliseconds.
        /// </summary>
        /// <remarks>
        /// NONE, and for the reason <see cref="TestTimeBudgetMs"/> is put out of reach
        /// rather than switched off - only more so. A wall around the menu does not make an
        /// option give up sooner, it decides which options are searched AT ALL, and it
        /// decides that from how long the machine took over the options before them. A suite
        /// running under one would mark a different set of options on a busy machine than on
        /// an idle one, which is a suite that flips on load. The shipped default is three
        /// seconds and the worst menu measured is two, so what a suite would be testing is
        /// the gap between those two numbers on whatever hardware it ran on.
        /// </remarks>
        public const int TestMenuTimeBudgetMs = 0;

        /// <summary>
        /// The look-ahead memory budget every suite runs with, in megabytes.
        /// </summary>
        /// <remarks>
        /// The shipped default, so what a suite measures is what a player gets. A suite
        /// that wants a crawl to run out reaches for
        /// <see cref="TestStateBudgetSetting"/> instead, which is the only budget that can
        /// stop one before it has looked at anything.
        /// </remarks>
        public const int TestMemoryBudgetMb = 256;

        /// <summary>
        /// The suite key that starves a crawl: a state budget, and TEST-ONLY.
        /// </summary>
        /// <remarks>
        /// <para>NOT A CONFIGURATION SETTING, and deliberately not spelled like one.
        /// <c>LookAheadStateBudget</c> was a player setting and is gone (de-7z0f) - a count
        /// of search states is not a quantity anybody outside this repository can reason
        /// about. What remains is a knob that reaches the mod only through the probe's
        /// prepare-suite command, which is where a suite's settings go; nothing here is
        /// written into the player's config file.</para>
        ///
        /// <para>THE MEMORY BUDGET CANNOT REPLACE IT, which was tried. It is checked when
        /// a node is dequeued, against a frontier that after seeding holds one state, and
        /// the group behind these scenarios carries twelve slots - about 96 bytes a state,
        /// so a megabyte holds eleven thousand of them and the crawl is finished long
        /// before the first check. A megabyte is the smallest a player can express. A
        /// state budget of one is compared against a frontier that already holds the seed,
        /// so it stops the search having looked at nothing.</para>
        ///
        /// <para>What a starved suite has to distinguish is GAVE UP from FOUND NOTHING,
        /// which is the whole point of the grey marker and of de-pvq.</para>
        /// </remarks>
        public const string TestStateBudgetSetting = "TestStateBudget";

        /// <summary>
        /// The suite key that stops the mod replacing a killed engine, and TEST-ONLY.
        /// </summary>
        /// <remarks>
        /// <para>NOT A CONFIGURATION SETTING, for the same reason as
        /// <see cref="TestStateBudgetSetting"/>: it reaches the mod only through the
        /// probe's prepare-suite command, and nothing here is written into the player's
        /// config file.</para>
        ///
        /// <para>WHAT IT IS FOR. Since de-bnjy.1.3 a dead engine is REPLACED - the shipped
        /// policy tolerates five deaths before the look-ahead gives up for the session - so
        /// a suite that kills one engine and waits for the shutdown notice would wait for
        /// ever. Zero means never replace, which is exactly the behaviour de-bnjy.1.2
        /// shipped, and it puts the give-up path one kill away instead of six.</para>
        ///
        /// <para>THE HONEST ALTERNATIVE WAS CONSIDERED AND IS A SEPARATE TASK: kill six
        /// engines with the shipped limit, winning a race with each replacement as it comes
        /// up. That measures the counter as well as the notice, and it needs harness
        /// machinery that does not exist yet. This setting buys back the notice coverage
        /// the recovery change would otherwise have cost, and no more than that.</para>
        /// </remarks>
        public const string TestRecoveryLimitSetting = "TestRecoveryLimit";

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

        /// <summary>Siileng's menu as a working engine marks it, at a balance of 5,100.</summary>
        /// <remarks>
        /// SHARED BY THE DEATH AND RECOVERY SUITES, which both open on it and both need it
        /// to mean the same thing: it is the "before" that makes an unmarked menu afterwards
        /// a CHANGE rather than a claim on its own, and in the recovery suite it is also the
        /// "after" that says the markers came back. Three copies of it would be three
        /// chances for one to drift.
        /// </remarks>
        private static readonly OptionExpectation[] MarkedSiilengMenu =
        {
            new OptionExpectation(
                BuySneakersEntry, Marker.Orange, "buying the sneakers leads on to the speakers"),
            new OptionExpectation(
                InspectSneakersEntry, Marker.Orange, "looking returns to the hub, which still can"),
            new OptionExpectation(
                InspectSpeakersEntry, Marker.Orange, "and so does looking at the other"),
            new OptionExpectation(LeaveEntry, Marker.None, "leaving reaches nothing at all"),
        };

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

        /// <summary>
        /// Klaasje's flower, thrown off the Whirling's roof - the one place in the game
        /// where a RED CHECK is the first thing a menu offers.
        /// </summary>
        /// <remarks>
        /// <para>Conversation 656, which the index calls WHIRLING ROOF ORB / handeye catch.
        /// The name to use for it is KLAASJE'S FLOWER: that is what the check is about and
        /// what its own flag says - <c>whirling.klaasje_flower_red_check_grab</c> - and
        /// "the orb" invites it being read as somewhere else in the Whirling. Its shape,
        /// read off the shipped index: 0 START to 1, 1 to 27, 27 is one narration line, and
        /// 27 leads to exactly two entries - 3, "Move your hand, fast!", carrying
        /// DifficultyRed, and 7, "[Discard chance.]".</para>
        ///
        /// <para>EVERY GUARD ON THAT PATH IS EMPTY, so the check appears from any save and
        /// nothing about where the player stands decides whether it is offered. IT STANDS AT
        /// THE FLOWER ANYWAY. It borrowed the ceiling fan's save while that was the only
        /// thing available, which made every one of its suites report under the name
        /// `at-the-fan` and read as though the fan's conversation were under test. A
        /// scenario should stand where its conversation happens even when it need not.</para>
        ///
        /// <para>de-8hh2.9 recorded that no red check was in an opening menu, having looked
        /// at the four conversations the harness already had saves for. The game has 111
        /// red-check entries across 76 conversations; this is one of them, and the variable
        /// its outcomes are sorted by names it -
        /// <c>whirling.klaasje_flower_red_check_grab</c>.</para>
        /// </remarks>
        /// <remarks>
        /// ZERO ADVANCES, measured rather than read off the index. Entry 27 is a narration
        /// line and the shape suggested one advance would be needed to get past it; the
        /// menu is in fact up as soon as the conversation is, which is what the run said
        /// and what the number here now records. Measured from the fan's save and checked
        /// again from the flower's, since a count of advances is a property of the
        /// conversation rather than of where the player is standing - and a claim like that
        /// is worth one run rather than an argument.
        /// </remarks>
        public static Somewhere KlaasjeFlower { get; } =
            new Somewhere("at-klaasjes-flower", 656, "Klaasje's flower, off the Whirling's roof", 0);

        /// <summary>
        /// A global state with nothing recorded in it, so everything is unseen anywhere.
        /// </summary>
        /// <remarks>
        /// Shared with the branch-shape rows, which name it in
        /// <c>testing/scenarios/branch-shapes.json</c>. The simplest fixture there is: no
        /// entry has been read, so nothing about the recording can explain what a marker
        /// turns out to be.
        /// </remarks>
        private const string EmptyState = "global-state-empty.json";

        /// <summary>The global state all three money scenarios share.</summary>
        /// <remarks>
        /// Every entry of the conversation except 80, so reaching 80 is the only way to
        /// find something no save has read.
        /// </remarks>
        private const string MoneyState = "global-conversation-state.json";

        /// <summary>Where PASSING the fan's check lands, and nothing else.</summary>
        /// <remarks>
        /// The pass half a rung below the fail half, with the check itself still on the top
        /// rung so its crawl is refused: red word, no asterisk, beside an orange one.
        /// </remarks>
        private const string FanPassOnlyState = "global-state-fan-pass-only.json";

        /// <summary>The fan's check itself, and nothing else.</summary>
        /// <remarks>
        /// Drops the CHECK a rung so its crawl runs, while leaving both outcomes where they
        /// were. Paired with a save that has read one outcome, it is what puts an asterisk
        /// on a dark red word.
        /// </remarks>
        private const string FanCheckRecordedState = "global-state-fan-check-recorded.json";

        /// <summary>Every entry of the fan, so nothing there is unseen anywhere.</summary>
        /// <remarks>
        /// The only way to a RED asterisk: the marker takes the best novelty beyond the
        /// destination, and while anything unseen-anywhere is reachable that best is orange.
        /// Removing the top rung from the conversation entirely is what leaves unseen-this-
        /// game as the best there is.
        /// </remarks>
        private const string FanAllRecordedState = "global-state-fan-all-recorded.json";

        /// <summary>at-the-fan, with the check's PASS outcome already read.</summary>
        /// <remarks>
        /// A SAVE AND NOT A FIXTURE FILE, because "already read" is the game's own per-save
        /// SimStatus and nothing staged beside the saves can set it. That is the whole
        /// reason a dark red word needs its own scenario save; see the sparse diff under
        /// testing/scenarios/fan-read-pass.ntwtf.
        /// </remarks>
        private const string FanReadPassSave = "fan-read-pass";

        /// <summary>The same, with the check itself read too.</summary>
        /// <remarks>
        /// Reading the CHECK drops it to the bottom rung, which is what leaves unseen-this-
        /// game able to outrank it once nothing anywhere is unseen.
        /// </remarks>
        private const string FanReadBothSave = "fan-read-both";

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
                Money, SeenElsewhere, SeenHere, Pristine, Budget, SwitchedOff, AllSeen,
                RedCheck,
            }.Concat(BranchShapes).Append(EngineRecovery).Append(EngineDeath).ToArray();

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
        /// <remarks>
        /// <para><see cref="EngineDeath"/> IS LAST, and both lists say so rather than
        /// leaving it to <see cref="InRunOrder"/>. It kills the look-ahead engine and tells
        /// the mod not to replace it, so every suite after it in the same launch sees a game
        /// with no look-ahead and fails every claim it makes about a marker or a line.
        /// Measured 2026-09-05: with it seventh, the eight branch-shape suites that follow
        /// lost all sixteen of their Pass / Fail claims, having passed the same claims
        /// twenty lines earlier under <see cref="Pristine"/>. de-pszk narrowed that from
        /// "for the rest of the launch" to "until the next engine is up" - a suite prepare
        /// now revives an engine its predecessor killed - but the engine comes up BEHIND the
        /// prepare, so the menus drawn first still have none, and last is still where this
        /// suite belongs.</para>
        ///
        /// <para><see cref="EngineRecovery"/> SITS JUST BEFORE IT AND IS SAFE THERE, which
        /// is the difference between the two: it kills an engine and then waits for the
        /// replacement, so it hands the next suite a working one. It is still after the
        /// ordinary suites because a suite that kills anything belongs with the ones that
        /// do - if its recovery ever stopped working, everything after it would fail the way
        /// the branch shapes did above, and grouping the killers keeps that blast radius
        /// where a reader expects it.</para>
        /// </remarks>
        public static IReadOnlyList<LookAheadSuite> Default =>
            new[]
            {
                Money, SeenElsewhere, SeenHere, Pristine, Budget, SwitchedOff,
            }.Concat(BranchShapes).Append(EngineRecovery).Append(EngineDeath).ToArray();

        /// <summary>
        /// The suites in the order they can actually be run: anything that ends the
        /// session's look-ahead goes last, and everything else keeps its place.
        /// </summary>
        /// <remarks>
        /// <para>THE DECLARED LISTS ARE ALREADY IN THIS ORDER, so for a default run this
        /// changes nothing. It is for the order a person types - <c>--suite
        /// engine-death,red-check</c> is a reasonable thing to ask for and a false failure
        /// without this, since the second suite would be drawing its first menus while the
        /// engine the first one killed is still being replaced.</para>
        ///
        /// <para>Reordering rather than refusing, because the request is not ambiguous:
        /// every named suite still runs, and the only thing decided here is which of them
        /// can still be believed. The caller says so out loud when the order changes.</para>
        ///
        /// <para>Stable within each half, so a run's output stays in the order it was asked
        /// for apart from the one suite that has to move.</para>
        /// </remarks>
        /// <param name="suites">The suites to run, in the order they were asked for.</param>
        /// <returns>The same suites, with the session-ending ones last.</returns>
        /// <exception cref="ArgumentNullException"><paramref name="suites"/> is null.</exception>
        public static IReadOnlyList<LookAheadSuite> InRunOrder(
            IReadOnlyList<LookAheadSuite> suites)
        {
            if (suites == null)
            {
                throw new ArgumentNullException(nameof(suites));
            }

            return suites.Where(suite => !suite.KillsTheEngine)
                .Concat(suites.Where(suite => suite.KillsTheEngine))
                .ToArray();
        }

        /// <summary>
        /// The forward scan spends as it walks.
        /// </summary>
        /// <remarks>
        /// DEFINED IN <c>testing/scenarios/suites.json</c>, which is also what
        /// <c>tests/scenario_suites.rs</c> runs. The three balances, the entries and the
        /// argument for them are all there; what is left here is the name.
        /// </remarks>
        public static LookAheadSuite Money => FromDefinition("money");

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
        /// DEFINED IN <c>testing/scenarios/suites.json</c>, which is also what
        /// <c>tests/scenario_suites.rs</c> runs. The four options and why leaving is the
        /// one that must NOT read as uncertain are stated there, as is why the overflow
        /// log stays an in-game check.
        /// </remarks>
        public static LookAheadSuite Budget => FromDefinition("budget");

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
        ///
        /// <para>THE ONE SUITE STILL DECLARED HERE, and the only one that should be. What
        /// it stages is a MOD SETTING, and there is no such thing to stage without a mod:
        /// the offline engine has no switch to turn off, so from the same fixture it
        /// answers exactly what the money suite's first scenario answers - three options
        /// marked. A row saying every option is unmarked would therefore be a definition
        /// only one side could execute, which is worse than an honest declaration because
        /// it reads as shared. Everything the two executors can BOTH run is in
        /// <c>testing/scenarios/suites.json</c>.</para>
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
                // WRITTEN ONCE AT LOAD, before any suite was prepared, so this
                // one asks about the run rather than about this suite.
                new LogExpectation(
                    "dialogue statuses are being tracked",
                    true,
                    "tracking is unaffected by the switch",
                    wholeRun: true),
            });

        /// <summary>
        /// A RED check on screen, which no scenario had ever put there.
        /// </summary>
        /// <remarks>
        /// <para>de-8hh2.9's option (2), which turned out to exist: a save that stands
        /// where a red check is the first thing offered. See <see cref="KlaasjeFlower"/>
        /// for the shape of conversation 656.</para>
        ///
        /// <para>IN <c>testing/scenarios/suites.json</c> NOW, which is where it always
        /// belonged. It was declared in code while it claimed nothing - markers and the
        /// Pass / Fail line both ignored, because nothing here had ever been on screen and
        /// this repository's rule is measure, do not reason - and a row asserting nothing
        /// has nothing for the offline executor to run. Both have been looked at since, so
        /// it claims them.</para>
        ///
        /// <para>AND IT CLAIMS WHAT THE BRANCH-SHAPE ROWS DO NOT. They open this same
        /// conversation and take markers as <see cref="MarkerPolicy.NoneAnywhere"/>,
        /// because a row of that table is about the LINE and the option ids are not its
        /// subject. This names them - 3, the red check, and 7, which declines - so the menu
        /// having the shape the index describes is asserted somewhere rather than assumed
        /// by everything.</para>
        /// </remarks>
        public static LookAheadSuite RedCheck => FromDefinition("red-check");

        /// <summary>
        /// The engine dies and the game carries on, with the feature off and nothing else.
        /// </summary>
        /// <remarks>
        /// <para>THE FAILURE THE WHOLE OUT-OF-PROCESS ARRANGEMENT EXISTS TO SURVIVE, and a
        /// running game is the only place it can be provoked for real - de-bnjy.1.2.4. Two
        /// scenarios over the same save and the same conversation: the first with an engine,
        /// which marks three options, and the second after it has been killed, which marks
        /// none.</para>
        ///
        /// <para>THE PAIR IS THE POINT. Either half alone proves nothing. Markers before a
        /// kill could be markers a mod draws whatever happens; no markers after one could be
        /// a mod that never had an engine, which is why the harness refuses a kill that
        /// found no process. Together they say the feature was working, that it stopped, and
        /// that stopping was all that happened.</para>
        ///
        /// <para>DECLARED HERE RATHER THAN IN <c>testing/scenarios/suites.json</c>, for the
        /// reason <see cref="SwitchedOff"/> is: what it stages is the DEATH OF A PROCESS,
        /// and the offline executor has no process to kill. From the same fixture it would
        /// answer what the money suite answers, three options marked, so a row claiming
        /// none are would be a definition only one side could execute.</para>
        ///
        /// <para>The log expectations carry the other half of the promise. The warning is
        /// expected EXACTLY ONCE, because a message repeated on every response menu
        /// afterwards is worse than the silence it replaced; and the tracking line has to
        /// be there, because losing what the player has read when a search process died
        /// would turn a cosmetic failure into data loss.</para>
        /// </remarks>
        public static LookAheadSuite EngineDeath { get; } = new LookAheadSuite(
            "engine-death",
            "an engine that dies takes the markers with it and nothing else",
            MoneyState,
            new[]
            {
                new LookAheadScenario(
                    "afford-both",
                    SiilengConversation,
                    "with an engine, the balance that marks three options",
                    MarkedSiilengMenu,
                    money: 5100,
                    advances: SiilengAdvances,
                    branchPolicy: BranchPolicy.NoneAnywhere),
                new LookAheadScenario(
                    "afford-both",
                    SiilengConversation,
                    "the same menu with the engine killed underneath it",
                    AllUnmarked("the look-ahead engine has gone"),
                    money: 5100,
                    advances: SiilengAdvances,
                    branchPolicy: BranchPolicy.NoneAnywhere,
                    killEngineFirst: true),
            },
            logExpectations: new[]
            {
                new LogExpectation(
                    "the look-ahead engine has gone and will not be restarted",
                    true,
                    "the mod says the engine has gone, once and only once",
                    times: 1),
                // THE NOTICE IS PIXELS, and pixels are not something a log check can read.
                // What this proves is that it was RAISED, through one of the game's own
                // channels, exactly once - the same once as the line above, since both
                // hang off the same guard. Whether it was legible is a question for a
                // screenshot, and the run takes one.
                new LogExpectation(
                    "the player was told on screen",
                    true,
                    "the player is told in the game, once and only once",
                    times: 1),
                // AND WHICH CHANNEL, because the mod has two and they are not equally
                // good. The window waits for the player; the notification it falls back to
                // passes by in a couple of seconds, on one unwrapped line that was clipped
                // at both ends at 1280 wide (de-gbl3). Without this the fallback would pass
                // the check above and look exactly like success.
                new LogExpectation(
                    "the player was told on screen in a window",
                    true,
                    "and told in a window, not the notification it falls back to",
                    times: 1),
                // WRITTEN ONCE AT LOAD, before any suite was prepared, so this
                // one asks about the run rather than about this suite.
                new LogExpectation(
                    "dialogue statuses are being tracked",
                    true,
                    "tracking survives an engine that died",
                    wholeRun: true),
                // THE MOD MUST NOT HAVE QUIETLY REPLACED IT. With the shipped policy one
                // kill produces a replacement and no notice at all, and this suite would
                // then be waiting on a window that was never raised. Asserting the absence
                // of the respawn line proves the limit above actually reached the mod,
                // rather than the suite passing for some other reason.
                new LogExpectation(
                    "the look-ahead engine has gone and a replacement is being started",
                    false,
                    "and it was not quietly replaced, which this suite turns off"),
            },
            pluginSettings: new Dictionary<string, string>
            {
                // NEVER REPLACE A KILLED ENGINE, which is what puts the shutdown notice one
                // kill away. See TestRecoveryLimitSetting for why the suite buys the
                // give-up path this way rather than killing six engines.
                [TestRecoveryLimitSetting] = "0",
            });

        /// <summary>
        /// An engine that dies is REPLACED, and the markers come back with it.
        /// </summary>
        /// <remarks>
        /// <para>The other half of <see cref="EngineDeath"/>, and the shipped behaviour -
        /// that suite turns the replacement OFF to reach the shutdown notice, so without
        /// this one nothing in the game exercises what actually happens when an engine dies
        /// (de-bnjy.1.3, de-wncd.3).</para>
        ///
        /// <para>THE THREE SCENARIOS ARE ONE ARGUMENT, in order:</para>
        ///
        /// <para>1. With an engine, the balance that marks three options - the same opening
        /// as the death suite, so that what follows is a CHANGE from something known rather
        /// than a claim on its own.</para>
        ///
        /// <para>2. The same menu with the engine killed underneath it. EVERY OPTION
        /// UNCERTAIN - a search really did run here and really did not finish, which is what
        /// '*?' means (de-pvq); drawing nothing would say "there is nothing unread down
        /// there" on the strength of a crash. The markers are not the real ones because the
        /// replacement is deliberately not built inside the frame that draws a response menu
        /// - a process launch plus a 173-244 ms index read has to happen behind it. This is
        /// also the scenario that would hang without <c>expectsRecovery</c>: it waits for the
        /// new engine instead of for a modal notice that is never raised.</para>
        ///
        /// <para>3. THE SAME MENU AGAIN, MARKED. This is the one that makes the suite worth
        /// running: it says the respawn is a RECOVERY rather than a quieter failure. Every
        /// scenario before it would pass just as well against a mod that had given up
        /// silently.</para>
        ///
        /// <para>NO RECOVERY LIMIT IS SET, unlike the death suite - the point is the SHIPPED
        /// policy, which tolerates five deaths and so answers this single kill with a new
        /// engine.</para>
        /// </remarks>
        public static LookAheadSuite EngineRecovery { get; } = new LookAheadSuite(
            "engine-recovery",
            "an engine that dies is replaced, and the markers come back",
            MoneyState,
            new[]
            {
                new LookAheadScenario(
                    "afford-both",
                    SiilengConversation,
                    "with an engine, the balance that marks three options",
                    MarkedSiilengMenu,
                    money: 5100,
                    advances: SiilengAdvances,
                    branchPolicy: BranchPolicy.NoneAnywhere),
                new LookAheadScenario(
                    "afford-both",
                    SiilengConversation,
                    "the menu drawn while the replacement is still coming up",
                    AllUncertain("a search ran here and the engine died before it answered"),
                    money: 5100,
                    advances: SiilengAdvances,
                    branchPolicy: BranchPolicy.NoneAnywhere,
                    killEngineFirst: true,
                    expectsRecovery: true),
                new LookAheadScenario(
                    "afford-both",
                    SiilengConversation,
                    "and the same menu once it is, marked again",
                    MarkedSiilengMenu,
                    money: 5100,
                    advances: SiilengAdvances,
                    branchPolicy: BranchPolicy.NoneAnywhere),
            },
            logExpectations: new[]
            {
                new LogExpectation(
                    "the look-ahead engine has gone and a replacement is being started",
                    true,
                    "the mod says it is replacing the engine, once",
                    times: 1),
                new LogExpectation(
                    "a replacement look-ahead engine is up",
                    true,
                    "and says the replacement arrived"),
                // THE POINT OF THE WHOLE SUITE, stated as an absence. If this line appears
                // the mod gave up rather than recovered, and scenario 2 would have passed
                // anyway because a mod that has given up also draws nothing.
                new LogExpectation(
                    "the look-ahead engine has gone and will not be restarted",
                    false,
                    "and never gives up, which one death must not cause"),
                // TOLD, BUT NOT STOPPED. The player sees '*?' on every option and needs
                // to know why, so a passing line says the engine crashed and is
                // restarting - while the modal window, which is reserved for the one fatal
                // message, must NOT appear.
                new LogExpectation(
                    "the player was told the engine is restarting",
                    true,
                    "the player is told the engine is restarting, once",
                    times: 1),
                new LogExpectation(
                    "the player was told the engine is restarting in a passing notification",
                    true,
                    "and told in passing rather than in a window that stops the game"),
                new LogExpectation(
                    "the player was told on screen",
                    false,
                    "and never gets the fatal notice, which is a different message"),
                // WRITTEN ONCE AT LOAD, before any suite was prepared, so this
                // one asks about the run rather than about this suite.
                new LogExpectation(
                    "dialogue statuses are being tracked",
                    true,
                    "tracking is unaffected throughout",
                    wholeRun: true),
            });

        /// <summary>
        /// Reaching a line another save has read, from an option this one has, is red.
        /// </summary>
        /// <remarks>
        /// DEFINED IN <c>testing/scenarios/suites.json</c>, which is also what
        /// <c>tests/scenario_suites.rs</c> runs. The ladder this needs both halves of, and
        /// why 86 is the entry that makes it more than a colour check, are stated there.
        /// </remarks>
        public static LookAheadSuite SeenElsewhere => FromDefinition("seen-elsewhere");

        /// <summary>
        /// A conversation this save has read to the end earns nothing.
        /// </summary>
        /// <remarks>
        /// DEFINED IN <c>testing/scenarios/suites.json</c>, which is also what
        /// <c>tests/scenario_suites.rs</c> runs. The bottom rung, and why the statistics
        /// artefact is what separates the shortcut from a crawl that found nothing, are
        /// stated there.
        /// </remarks>
        public static LookAheadSuite SeenHere => FromDefinition("seen-here");

        /// <summary>
        /// An option that is itself unread anywhere is never marked, and never crawled.
        /// </summary>
        /// <remarks>
        /// DEFINED IN <c>testing/scenarios/suites.json</c>, which is also what
        /// <c>tests/scenario_suites.rs</c> runs. The four conversations, why each was
        /// picked, and why the statistics artefact asserts no STATE rather than no crawl
        /// are stated there.
        /// </remarks>
        public static LookAheadSuite Pristine => FromDefinition("pristine");


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
        /// DEFINED IN <c>testing/scenarios/suites.json</c>, which is also what
        /// <c>tests/scenario_suites.rs</c> runs. What this suite used to measure, why the
        /// short-circuit ended that, and why the five biggest conversations are still the
        /// right scenarios for the claim are stated there.
        /// </remarks>
        public static LookAheadSuite AllSeen => FromDefinition("all-seen");

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

        /// <summary>
        /// One suite per row of the shared scenario definition.
        /// </summary>
        /// <remarks>
        /// <para>DECLARED IN DATA RATHER THAN HERE, because the same scenarios are run
        /// without a game by <c>tests/branch_shapes.rs</c>, and a definition that lived in
        /// this file could only be mirrored there rather than shared. See
        /// <see cref="BranchShapeTable"/> for the argument; the short form is that an
        /// offline test which has drifted from the run it stands in for is fast, green, and
        /// about a scenario nobody runs.</para>
        ///
        /// <para>WHAT A ROW BECOMES: a suite of one scenario, staging the row's global
        /// state, loading its save, opening the conversation its check names, and requiring
        /// the row's line on every rolled check in the menu - which is that one check and
        /// nothing else, in both menus these rows open. Markers are claimed too, as NONE
        /// anywhere: a rolled check draws no marker of its own (de-8hh2.2), and every other
        /// option in these menus is refused a crawl in every one of these fixtures.</para>
        ///
        /// <para>TWO CHECKS, EIGHT SHAPES EACH. The ceiling fan's check is WHITE and the
        /// whirling roof orb's is RED, and the mod draws one line of code onto both bands -
        /// so a shape only ever arranged on the fan was a shape nobody had seen on a red
        /// check. See <see cref="KlaasjeFlower"/> for why any save reaches the orb.</para>
        ///
        /// <para>A ROW'S BUDGET, where it names one, is applied exactly as the budget suite
        /// applies its own - see the remarks there on why a state count is the only limit
        /// that gives up at the same point on every machine.</para>
        /// </remarks>
        public static IReadOnlyList<LookAheadSuite> BranchShapes => _branchShapes.Value;

        /// <summary>One suite out of <c>testing/scenarios/suites.json</c>, by name.</summary>
        /// <remarks>
        /// <para>THE DEFINITION IS THE FILE. What a suite stages, which saves it opens and
        /// what each option must carry are all written there, and
        /// <c>tests/scenario_suites.rs</c> reads the same rows - so a fixture cannot drift
        /// between the run and the offline check, because there is only one of it.</para>
        ///
        /// <para>Read once and cached, because a suite is asked for repeatedly - the
        /// selection code walks <see cref="All"/> - and re-parsing per ask would also mean
        /// re-throwing per ask, which turns one clear failure at startup into a scatter of
        /// them.</para>
        /// </remarks>
        /// <param name="name">The suite's name, as the file spells it.</param>
        /// <returns>The suite.</returns>
        /// <exception cref="InvalidDataException">There is no such suite.</exception>
        public static LookAheadSuite FromDefinition(string name)
        {
            if (!_defined.Value.TryGetValue(name, out LookAheadSuite? suite))
            {
                throw new InvalidDataException(
                    $"'{name}' is not a suite in {ScenarioTable.FileName}. The ones there "
                    + "are: "
                    + string.Join(", ", _defined.Value.Keys.OrderBy(k => k, StringComparer.Ordinal))
                    + ".");
            }

            return suite;
        }

        private static readonly Lazy<IReadOnlyDictionary<string, LookAheadSuite>> _defined =
            new Lazy<IReadOnlyDictionary<string, LookAheadSuite>>(BuildDefined);

        /// <summary>
        /// The suites the definition file carries, minus the ones it says are switched off.
        /// </summary>
        /// <remarks>
        /// A DISABLED SUITE IS STILL DECLARED and still validated - its rows are read, its
        /// markers are checked for spelling, and it is one word away from running again -
        /// but it is not built, so no run does it and no default set includes it. The
        /// reason it gives is printed, because a green run that quietly does less than it
        /// did is worse than a red one.
        /// </remarks>
        private static IReadOnlyDictionary<string, LookAheadSuite> BuildDefined()
        {
            var built = new Dictionary<string, LookAheadSuite>(StringComparer.Ordinal);
            foreach (ScenarioSuiteDefinition definition in ScenarioTable.Read().Suites)
            {
                if (!string.IsNullOrWhiteSpace(definition.Disabled))
                {
                    Console.WriteLine(
                        $"suites:    '{definition.Suite}' is switched off: {definition.Disabled}");
                    continue;
                }

                built[definition.Suite] = definition.Build(ArtefactChecks);
            }

            return built;
        }

        /// <summary>The artefact predicates a suite may name, by the name it uses.</summary>
        /// <remarks>
        /// THE HALF OF A SUITE THAT CANNOT BE WRITTEN DOWN. Each of these parses a file the
        /// mod left behind and says whether it adds up, which is code and belongs in code;
        /// what a definition can carry is which one to run. A name with nothing behind it
        /// is refused when the table is read rather than when the run reaches the check,
        /// so a typo costs a message and not a launch.
        /// </remarks>
        private static IReadOnlyDictionary<string, Func<string?, string?>> ArtefactChecks =>
            new Dictionary<string, Func<string?, string?>>(StringComparer.Ordinal)
            {
                ["statisticsAddUp"] = CheckStatistics,
                ["noCrawls"] = NoCrawls,
                ["overflowNamesTheConversation"] = CheckOverflowLog,
                ["reportOverflows"] = ReportOverflows,
            };

        private static readonly Lazy<IReadOnlyList<LookAheadSuite>> _branchShapes =
            new Lazy<IReadOnlyList<LookAheadSuite>>(BuildBranchShapes);

        private static IReadOnlyList<LookAheadSuite> BuildBranchShapes()
        {
            BranchShapeTable table = BranchShapeTable.Read();
            return table.Checks
                .SelectMany(check => check.Rows.Select(row => new LookAheadSuite(
                    row.Suite,
                    $"{row.What}, on {check.What}",
                    row.State,
                    new[]
                    {
                        new LookAheadScenario(
                            row.Save,
                            check.Conversation,
                            row.What,
                            Array.Empty<OptionExpectation>(),
                            markers: MarkerPolicy.NoneAnywhere,
                            advances: PlaceOf(check.Conversation).Advances,
                            branchPolicy: BranchPolicy.EveryCheck,
                            branches: new BranchExpectation(
                                row.Pass.Expected(),
                                row.Fail.Expected(),
                                $"{row.What} - so Pass is {row.Pass} and Fail is {row.Fail}")),
                    },
                    pluginSettings: row.StateBudget > 0
                        ? new Dictionary<string, string>
                        {
                            [TestStateBudgetSetting] =
                                row.StateBudget.ToString(CultureInfo.InvariantCulture),
                        }
                        : null)))
                .ToArray();
        }

        /// <summary>The place a branch-shape check is opened from, by its conversation.</summary>
        /// <remarks>
        /// WHAT THE DEFINITION CANNOT CARRY. A row names its conversation, and how many
        /// lines of narration stand between opening that conversation and its first menu is
        /// a MEASURED property of the place - see <see cref="Somewhere.Advances"/> - which
        /// already has one home. Looking it up here rather than repeating the number in
        /// <c>branch-shapes.json</c> keeps the two from disagreeing, and a conversation with
        /// no place behind it is refused when the table is read rather than when the run
        /// reaches it.
        /// </remarks>
        /// <param name="conversation">The conversation a check opens.</param>
        /// <returns>Where it is opened from.</returns>
        /// <exception cref="InvalidDataException">Nothing here stands in that place.</exception>
        private static Somewhere PlaceOf(int conversation) => conversation switch
        {
            9 => CeilingFan,
            656 => KlaasjeFlower,
            _ => throw new InvalidDataException(
                $"{BranchShapeTable.FileName} names conversation {conversation}, and no "
                + "place here opens it - so nobody has measured how much narration stands "
                + "in front of its first menu."),
        };

        private static OptionExpectation Unmarked(int entryId, string why) =>
            new OptionExpectation(entryId, Marker.None, why);

        /// <summary>Every option Siileng's hub offers, expected to carry nothing.</summary>
        /// <remarks>
        /// The last of these helpers, and the only suite left that needs one. Every other
        /// scenario's options are written out in
        /// <c>testing/scenarios/suites.json</c>; <see cref="SwitchedOff"/> is not, because
        /// what it stages is a mod setting and there is no such thing to stage without a
        /// mod - see the remarks there.
        /// </remarks>
        private static OptionExpectation[] AllUnmarked(string why) => new[]
        {
            Unmarked(BuySneakersEntry, why),
            Unmarked(InspectSneakersEntry, why),
            Unmarked(InspectSpeakersEntry, why),
            Unmarked(LeaveEntry, why),
        };

        /// <summary>Every option Siileng's hub offers, expected to carry the grey '*?'.</summary>
        /// <remarks>
        /// WHAT A MENU GETS WHEN THE ENGINE DIED UNDER IT. Distinct from
        /// <see cref="AllUnmarked"/> in exactly the way <c>Marker.None</c> is distinct from
        /// <c>Marker.Uncertain</c>: none is an answer, and this is the absence of one. A
        /// suite that could not tell them apart would read a crash as "nothing unread here".
        /// </remarks>
        private static OptionExpectation[] AllUncertain(string why) => new[]
        {
            new OptionExpectation(BuySneakersEntry, Marker.Uncertain, why),
            new OptionExpectation(InspectSneakersEntry, Marker.Uncertain, why),
            new OptionExpectation(InspectSpeakersEntry, Marker.Uncertain, why),
            new OptionExpectation(LeaveEntry, Marker.Uncertain, why),
        };
    }
}
