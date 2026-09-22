// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using GlobalConversationTracker.Engine;
using GlobalConversationTracker.Session;
using Il2CppInterop.Runtime;
using Il2CppInterop.Runtime.InteropTypes.Arrays;
using HarmonyLib;
using PixelCrushers.DialogueSystem;
using UnityEngine;

namespace GlobalConversationTracker
{
    /// <summary>
    /// The third display hook: an option that can still lead somewhere the player has
    /// not read gets a trailing coloured asterisk.
    /// </summary>
    /// <remarks>
    /// <para>The colour says what is reachable, and the marker only appears when that is
    /// better than the option's own state - an option that is itself unseen-anywhere
    /// never gets one, because nothing outranks where it already leads.</para>
    ///
    /// <para><c>Sunshine.ConversationLogger.ChooseResponseText</c> is the hook: the one
    /// funnel every response's displayed text passes through, whatever kind of node it
    /// is. Its six special cases - Janus, white check, red check, passive, cost, fake
    /// check - all return through it, so a postfix here reaches every option exactly
    /// once, and reaches it after the game has finished composing the text.</para>
    ///
    /// <para>Read-only with respect to the game: it appends to a string. The crawl runs
    /// entirely over the mod's own copy of the graph and its own state vector, and
    /// touches Lua only to read. A failure costs the marker and nothing else.</para>
    /// </remarks>
    internal static class ResponseLookAheadPatch
    {
        /// <summary>
        /// What every line about the look-ahead starts with.
        /// </summary>
        /// <remarks>
        /// Was <c>BridgeComparison.LogPrefix</c>. The comparison it named is gone - there is
        /// only one engine to compare against now - but the prefix outlived it VERBATIM,
        /// because the harness greps the game's log for this exact string to find out what
        /// a run did. Changing the words breaks the runs, not the mod.
        /// </remarks>
        internal const string LogPrefix = "Look-ahead bridge:";

        /// <summary>What has happened, said once and in one place.</summary>
        /// <remarks>
        /// Shared by the log line and the on-screen notice so the two cannot drift apart.
        /// GREPPED BY THE HARNESS, like <see cref="LogPrefix"/>: the engine-death suite
        /// matches this exact wording and counts it, so changing the words breaks the runs.
        /// </remarks>
        internal const string EngineHasGone =
            "the look-ahead engine has gone and will not be restarted";

        /// <summary>What has happened when there is still budget to replace it.</summary>
        /// <remarks>
        /// DELIBERATELY NOT A PREFIX OF <see cref="EngineHasGone"/> PAST THE FIRST WORDS.
        /// That one says "will not be restarted" and the engine-death suite counts it as
        /// the end of the feature; a respawn is the opposite claim, and a run that could
        /// not tell the two apart would score every recovery as a shutdown. GREPPED BY THE
        /// HARNESS in its own right, as is <see cref="EngineReplacementUp"/>, which is the
        /// other half of the story - this line says one was started, that one says it
        /// arrived, and a run wants to know which happened.
        /// </remarks>
        internal const string EngineReplaced =
            "the look-ahead engine has gone and a replacement is being started";

        /// <summary>Said when the replacement has arrived and markers resume.</summary>
        internal const string EngineReplacementUp =
            "a replacement look-ahead engine is up";

        /// <summary>What the player is told, in passing, about a crash being recovered from.</summary>
        /// <remarks>
        /// ONE SHORT LINE, because the channel is one unwrapped line that passes by in a
        /// couple of seconds - de-gbl3 measured the fatal notice losing a word off each end
        /// at 1280 wide, and this has to fit where that did not.
        /// </remarks>
        internal const string RecoveryLine =
            "Look-ahead engine crashed; restarting. Options show '*?' meanwhile.";

        /// <summary>Said when the player has been told about a RECOVERABLE crash.</summary>
        /// <remarks>
        /// DELIBERATELY NOT CONTAINING <see cref="NoticeShown"/>. The harness counts that
        /// string to check the player was told the feature is GONE, exactly once; a
        /// recovery notice that contained it would be counted as a second shutdown and the
        /// engine-death suite would fail whenever a recovery had happened first in the same
        /// launch. Two different events, two different strings to grep.
        /// </remarks>
        internal const string RecoveryNoticeShown =
            "the player was told the engine is restarting";

        /// <summary>The one thing the player can do about it.</summary>
        internal const string RestartAdvice = "restart the game to bring the feature back";

        /// <summary>Said when the player has been told on screen, so a run can check.</summary>
        /// <remarks>
        /// The notice itself is pixels, and pixels are not something the harness reads. This
        /// line is the evidence that it was raised; the screenshot beside it is the evidence
        /// that it was legible. GREPPED BY THE HARNESS and counted, like the two above.
        /// </remarks>
        internal const string NoticeShown = "the player was told on screen";

        /// <summary>Which of the two channels carried it: the window, or the fallback.</summary>
        /// <remarks>
        /// <para>Appended to <see cref="NoticeShown"/> rather than replacing it, so the
        /// count of "was the player told" stays one line whichever channel answered, and a
        /// run can still ask the sharper question - WHICH channel - by matching the longer
        /// string. The engine-death suite matches both.</para>
        ///
        /// <para>The distinction is worth a word because the two are not equally good: one
        /// waits for the player, the other passes by in a couple of seconds. A run that
        /// silently got the passing one would report a success that looked like the
        /// success it was aiming at.</para>
        /// </remarks>
        internal const string NoticeInAWindow = "in a window";

        /// <summary>The fallback's half of <see cref="NoticeInAWindow"/>.</summary>
        internal const string NoticeInANotification = "in a passing notification";

        /// <summary>
        /// The thread <see cref="Install"/> ran on, which is the game's own.
        /// </summary>
        /// <remarks>
        /// Unity objects may only be touched from the main thread, and the notice touches
        /// several. Today the only path into <see cref="EngineDied"/> is the response menu
        /// being drawn, which IS this thread - but that is a fact about the current call
        /// sites rather than a guarantee, and a future one (the warm-up task can fail too)
        /// would otherwise crash the game from inside the handler for a crash. Compared
        /// rather than trusted, so an off-thread caller loses the notice and keeps the log.
        /// </remarks>
        private static int _mainThreadId = -1;

        private static GlobalStateSession? _session;
        private static HookFailureLimiter? _failures;
        private static string _unseenAnyGameHtml = NovelResponseColorPatch.DefaultNovelColorHtml;
        private static string _unseenThisGameHtml = DefaultUnseenThisGameColorHtml;
        private static string _uncertainHtml = DefaultUncertainColorHtml;
        private static string _seenHtml = DefaultSeenColorHtml;
        private static bool _markUncertain = true;
        private static LookAheadDiagnosticsWriter? _diagnostics;
        /// <summary>Test-only, set by the probe and by no configuration setting.</summary>
        private static int _stateBudget;
        private static int _timeBudgetMs;
        private static int _menuTimeBudgetMs;
        private static int _memoryBudgetMb;
        private static bool _enabled = true;
        private static IGlobalStateLog? _log;

        /// <summary>The engine and the index it reads, or null if there is none.</summary>
        /// <remarks>
        /// Opened once. Null afterwards means the engine or the index is not there, and
        /// the managed engine simply carries on.
        /// </remarks>
        private static LookAheadIndex? _bridge;
        private static bool _bridgeOpened;

        /// <summary>The open, started at load and running off the game's threads.</summary>
        /// <remarks>
        /// <para>OPENING COSTS MORE THAN IT DID. It was a fifteen-megabyte index parse,
        /// which is why it was made lazy - at the first response menu rather than at plugin
        /// load, so that it was not in the frame that draws the main menu. Since de-bnjy.1
        /// there is a process launch in front of that, and the same argument applies one
        /// level out: the cost moved, so the answer has to move with it.</para>
        ///
        /// <para>So it is started at load and done on a thread of its own, and a menu drawn
        /// before it finishes is answered by the managed engine - which is what a menu gets
        /// anyway when there is no index at all. Nobody waits: not the main menu, which is
        /// why this is not synchronous at load, and not the first conversation, which is why
        /// it is no longer lazy.</para>
        ///
        /// <para>THE TASK IS THE ONLY THING THAT TOUCHES WHAT IT IS BUILDING. Neither the
        /// engine nor the index it holds is thread-safe, so nothing reads the result until
        /// the task has finished and handed it over - see <see cref="Bridge"/>, which
        /// returns null rather than looking at a half-built one.</para>
        /// </remarks>
        private static Task<LookAheadIndex?>? _warming;

        /// <summary>Whether the engine has died for the LAST time, which is final.</summary>
        /// <remarks>
        /// Set once by <see cref="EngineDied"/> and, in play, never cleared. It is what
        /// makes the message appear once and the feature stay off - the hooks are removed as
        /// well, but a hook that is mid-call when the engine goes still has to find its way
        /// out. The two things that do clear it are both test-only:
        /// <see cref="Install"/>, which starts a session over, and
        /// <see cref="ReviveForSuite"/>, which hands the next in-game suite an engine.
        ///
        /// <para>NOT SET BY EVERY DEATH SINCE de-bnjy.1.3. Most deaths are answered with a
        /// fresh engine and no message at all; this is only the end of the road, when
        /// <see cref="_recovery"/> says there have been too many to keep paying for
        /// another.</para>
        /// </remarks>
        private static bool _engineDied;

        /// <summary>Whether to respawn, what to blame for a death, and when to stop.</summary>
        /// <remarks>
        /// de-bnjy.1.3. Replaced rather than reset, and by <see cref="Configure"/> - so on
        /// every suite prepare, not only on an install - so that an in-game suite does not
        /// inherit the deaths its predecessor caused on purpose.
        /// </remarks>
        private static EngineRecovery _recovery = new EngineRecovery();

        /// <summary>Whether the open now in flight is replacing an engine that died.</summary>
        /// <remarks>
        /// WHICH IS NOT THE SAME AS THE FIRST OPEN FAILING. An installation with no engine
        /// deployed at all fails to open every time and must go on quietly answering with
        /// the managed engine, so its failures must not spend the recovery budget and must
        /// never reach the give-up notice. Only a respawn's failure counts, which is what
        /// this distinguishes - see <see cref="Bridge"/>.
        /// </remarks>
        private static bool _respawning;

        /// <summary>Whether a crashed engine is being replaced right now.</summary>
        /// <remarks>
        /// <para>THE WINDOW IN WHICH NOTHING CAN BE ANSWERED BUT SOMETHING DID RUN. It
        /// opens when a death is answered with a respawn and closes when the replacement is
        /// promoted in <see cref="Bridge"/>, so it covers the menu the engine died on and
        /// every menu drawn before the new one is up.</para>
        ///
        /// <para>What a menu drawn inside it gets is the UNCERTAIN marker rather than
        /// nothing - see <see cref="MarkerFor"/>. Deliberately NOT the same as the warm-up
        /// before the first engine has ever opened: nothing has run then, and drawing
        /// nothing is the honest answer.</para>
        /// </remarks>
        private static bool Recovering => _respawning && !_engineDied;

        /// <summary>
        /// The look-ahead's OWN Harmony instance, so it can take its own hooks off.
        /// </summary>
        /// <remarks>
        /// Its own rather than the plugin's, decided in de-bnjy.1.2. Unpatching is by id,
        /// and the plugin's id covers the tracking hooks, the HUD counts and the option
        /// colours - none of which have anything to do with the engine, and one of which
        /// records what the player has read. An engine that dies must not cost that.
        /// </remarks>
        private static Harmony? _harmony;

        /// <summary>The id that instance is built from, or null before the first install.</summary>
        /// <remarks>
        /// KEPT BECAUSE THE INSTANCE IS NOT. <see cref="GiveUpOnLookAhead"/> unpatches and
        /// drops <see cref="_harmony"/>, so <see cref="ReviveForSuite"/> has nothing left to
        /// re-use; the id is the one piece that has to survive a give-up for the hooks to go
        /// back on under the same name. Null means <see cref="Install"/> has never run,
        /// which is the one case where there is nothing to revive.
        /// </remarks>
        private static string? _harmonyId;

        /// <summary>What the bridge said about the options of the menu being drawn.</summary>
        /// <remarks>
        /// <para>Filled once per menu, read once per option. This is the whole reason the
        /// bridge takes a list of starts: the world is the same for every option drawn at
        /// once and it is the world that is expensive to send, so one call amortises the
        /// marshalling over the menu instead of paying it per option.</para>
        ///
        /// <para>THE WHOLE RESPONSE RATHER THAN A DICTIONARY KEYED BY ENTRY, since
        /// de-8hh2.6. A rolled check comes back as TWO answers - one per outcome - so an
        /// entry no longer names one answer, and a dictionary keyed by entry would have
        /// kept whichever arrived last. Looking one up takes the entry AND the outcome,
        /// which is what <see cref="LookAheadResponse.Find"/> does.</para>
        /// </remarks>
        private static LookAheadResponse? _menuAnswers;

        /// <summary>
        /// Every entry the conversation in progress has stepped through, sent with every request.
        /// </summary>
        /// <remarks>
        /// Recorded here and interpreted by the engine - see <see cref="ConversationWalk"/>. It
        /// is cleared when a conversation starts and when it ends, so a walk always begins at a
        /// conversation's start.
        /// </remarks>
        private static readonly ConversationWalk _walk = new ConversationWalk();

        /// <summary>What the engine asks about a group, cached because it cannot change.</summary>
        private static readonly Dictionary<int, LookAheadQuestions> _questions =
            new Dictionary<int, LookAheadQuestions>();


        /// <summary>Which index the cached questions came from.</summary>
        private static int _questionsGeneration = -1;

        /// <summary>Where the mod keeps its own files, for a rebuilt index.</summary>
        private static string? _modDirectory;

        /// <summary>
        /// The colour for "leads to something no save has reached", matching the option
        /// colour the mod already paints such an option in.
        /// </summary>
        internal const string DefaultUnseenThisGameColorHtml = "#C4453C";

        /// <summary>
        /// The colour for "the search gave up before it could tell you".
        /// </summary>
        /// <remarks>
        /// Deliberately drab, and deliberately not either of the other two. The other
        /// markers are a promise - there IS something through here - and this one is the
        /// absence of a promise, so a colour that reads as a weaker version of either would
        /// be saying the wrong thing quietly.
        ///
        /// ON BLACK, which is where BOTH markers that use it are drawn. The Pass / Fail
        /// line under a check used to be the exception - it sat on the check's own band,
        /// where this grey measured 1.08:1 against a white check's #857F70 and was
        /// invisible - and it had a near-white colour of its own for that reason. It has
        /// not sat there since <see cref="CheckBandInsetPatch"/> began shortening the band
        /// to make room underneath, so the second colour was removed with the background
        /// that needed it (de-8hh2.4 is the measurement, and is now history).
        /// </remarks>
        internal const string DefaultUncertainColorHtml = "#7A7A7A";

        /// <summary>
        /// The colour for "already read", used on the Pass/Fail line only.
        /// </summary>
        /// <remarks>
        /// A DARK RED, and the third of the three that line paints its words in - see the
        /// design on de-fes. An option itself never needs this colour, because the game
        /// already draws a spent option in its own way and the mod leaves it alone. The
        /// line has to name the state explicitly: "Fail" in no colour at all would read as
        /// a missing answer rather than as a read one.
        /// </remarks>
        internal const string DefaultSeenColorHtml = "#7C2F2A";

        /// <summary>
        /// Appended to the plugin's Harmony id to make this patch's own.
        /// </summary>
        /// <remarks>
        /// Derived from the plugin's rather than written out, so the two cannot drift apart
        /// and so the look-ahead's id is recognisably the plugin's in any Harmony
        /// diagnostic that lists them.
        /// </remarks>
        private const string LookAheadPatchSuffix = ".lookahead";

        /// <summary>The marker for an option whose crawl finished and found something.</summary>
        private const string FoundMarker = "*";

        /// <summary>
        /// The marker for an option whose crawl ran out of budget without finding anything.
        /// </summary>
        private const string UncertainMarker = "*?";

        /// <summary>
        /// Applies the patch. Call once, from plugin load, after the session exists.
        /// </summary>
        /// <param name="harmony">
        /// The plugin's Harmony instance, used ONLY for its id. This patch installs through
        /// an instance of its own, derived from that id, so that it can take its own hooks
        /// off when the engine dies without touching the tracking hooks - see
        /// <see cref="EngineDied"/>.
        /// </param>
        /// <param name="session">The session a seen state is read from.</param>
        /// <param name="log">Where hook failures are reported.</param>
        /// <param name="modDirectory">
        /// Where the mod keeps its own files, so a stale index can be rebuilt into it.
        /// </param>
        /// <param name="unseenAnyGameHtml">Colour for reaching never-seen-anywhere text.</param>
        /// <param name="unseenThisGameHtml">Colour for reaching unseen-this-save text.</param>
        /// <param name="uncertainHtml">Colour for a crawl that gave up before it could say.
        /// Used on an option and on a check's Pass / Fail line alike.</param>
        /// <param name="markUncertain">Whether a crawl that gave up says so at all.</param>
        /// <param name="stateBudget">
        /// The most search states one option may hold, or 0 for no such limit. TEST-ONLY:
        /// no configuration setting writes it, and plugin load passes 0.
        /// </param>
        /// <param name="timeBudgetMs">
        /// The longest one option's crawl may run for, in milliseconds; 0 for no limit.
        /// </param>
        /// <param name="menuTimeBudgetMs">
        /// The longest the whole menu may run for, in milliseconds; 0 for no limit. Bounds
        /// the SUM the per-option budget above only bounds a term of - see de-dt75.3.
        /// </param>
        /// <param name="memoryBudgetMb">
        /// The most memory one option's crawl may hold, in megabytes; 0 for the engine's
        /// own default. The limit that normally decides - see de-e23q.
        /// </param>
        /// <param name="enabled">Whether the installed hook should add markers.</param>
        /// <param name="diagnostics">
        /// Where budget overflows and cost statistics are recorded, or null to record
        /// neither.
        /// </param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        internal static void Install(
            Harmony harmony,
            GlobalStateSession session,
            IGlobalStateLog log,
            string modDirectory,
            string unseenAnyGameHtml,
            string unseenThisGameHtml,
            string uncertainHtml,
            bool markUncertain,
            int stateBudget,
            int timeBudgetMs,
            int menuTimeBudgetMs,
            int memoryBudgetMb,
            bool enabled,
            LookAheadDiagnosticsWriter? diagnostics = null)
        {
            if (harmony == null)
            {
                throw new ArgumentNullException(nameof(harmony));
            }

            if (log == null)
            {
                throw new ArgumentNullException(nameof(log));
            }

            _session = session ?? throw new ArgumentNullException(nameof(session));
            _log = log;
            // Load runs on the game's own thread, so this is the thread the notice may be
            // raised from. Captured rather than assumed; see the field.
            _mainThreadId = Thread.CurrentThread.ManagedThreadId;
            _modDirectory = modDirectory
                ?? throw new ArgumentNullException(nameof(modDirectory));
            _failures = new HookFailureLimiter(
                "marking options that still lead somewhere unread", log);
            _unseenAnyGameHtml = Validate(unseenAnyGameHtml, nameof(unseenAnyGameHtml));
            _unseenThisGameHtml = Validate(unseenThisGameHtml, nameof(unseenThisGameHtml));
            _uncertainHtml = Validate(uncertainHtml, nameof(uncertainHtml));
            _markUncertain = markUncertain;
            Configure(
                enabled, stateBudget, timeBudgetMs, menuTimeBudgetMs, memoryBudgetMb,
                diagnostics);

            // Started now and finished elsewhere, so that neither the main menu nor the
            // first conversation waits for a process launch and a fifteen-megabyte parse.
            BeginOpening();

            // A SECOND INSTALL WOULD ORPHAN THE FIRST INSTANCE, and hooks nothing holds an
            // instance for cannot be taken off again. Load calls this once; a test may not.
            _harmony?.UnpatchSelf();
            _engineDied = false;
            _respawning = false;

            // ITS OWN ID, the plugin's with a suffix, and REMEMBERED rather than derived on
            // the spot: the give-up path drops the instance, so bringing the look-ahead
            // back for the next suite has to build another one from the same id.
            _harmonyId = harmony.Id + LookAheadPatchSuffix;
            PatchTheHooks();
        }

        /// <summary>Installs the look-ahead's two hooks, under an instance of its own.</summary>
        /// <remarks>
        /// SHARED BY <see cref="Install"/> AND <see cref="ReviveForSuite"/>, because
        /// <see cref="GiveUpOnLookAhead"/> takes both hooks off and throws the instance
        /// away. Whatever puts them back has to put back exactly what came off, and one
        /// place to do it is what keeps the two paths from drifting.
        /// </remarks>
        private static void PatchTheHooks()
        {
            // AN INSTANCE OF ITS OWN, whose id is the plugin's with a suffix. Unpatching is
            // by id, so hooks that share one cannot be removed separately - and these two
            // have to come off, when the engine dies, without taking the tracking hooks
            // with them.
            _harmony = new Harmony(_harmonyId);

            // Two hooks, and they are not interchangeable. The menu one is where the whole
            // list of options exists, which is the only place a single bridge call can
            // cover all of them; the text one is where a marker can be attached to an
            // option's own string.
            _harmony.PatchAll(typeof(ResponseMenuPatch));
            _harmony.PatchAll(typeof(ChooseResponseTextPatch));

            // AND FOUR THAT RECORD THE WALK every request carries: the two places the game steps
            // through an entry - the state it is about to display, and the links it follows past
            // the entries it displays nothing for - and a conversation starting or ending.
            _harmony.PatchAll(typeof(ConversationStatePatch));
            _harmony.PatchAll(typeof(EvaluateLinksAtPriorityPatch));
            _harmony.PatchAll(typeof(ConversationStartPatch));
            _harmony.PatchAll(typeof(ConversationEndPatch));
        }

        /// <summary>Changes suite-scoped behavior without reinstalling the hook.</summary>
        /// <param name="enabled">Whether to mark options at all.</param>
        /// <param name="stateBudget">The search-state budget, or 0 for none.</param>
        /// <param name="timeBudgetMs">The time budget in milliseconds, or 0 for none.</param>
        /// <param name="menuTimeBudgetMs">
        /// The whole menu's time budget in milliseconds, or 0 for none. NAMED BY EVERY
        /// CALLER for the reason the recovery limit below is replaced every time: a suite
        /// left with the last one's wall is measuring a policy it never asked for, and a
        /// wall is exactly the kind of thing that changes WHICH options give up.
        /// </param>
        /// <param name="memoryBudgetMb">
        /// The memory budget in megabytes, or 0 for the engine's own default.
        /// </param>
        /// <param name="diagnostics">Where to record crawls, or null to record none.</param>
        /// <param name="recoveryLimit">
        /// How many engine deaths to answer with a fresh engine before giving up for the
        /// session; negative asks for the shipped limit, which is what every caller that
        /// does not care about it passes. Either way the policy is replaced, so no caller
        /// inherits what the last one spent.
        /// </param>
        /// <remarks>
        /// THE RECOVERY LIMIT IS HERE FOR ONE REASON: an in-game suite cannot otherwise
        /// reach the give-up path. The default tolerates five deaths, so a suite that
        /// wanted to see the shutdown notice would have to kill six engines AND win a race
        /// with each replacement as it came up. Set to zero it never respawns - the
        /// behaviour de-bnjy.1.2 shipped - and one kill reaches the notice, which is what
        /// the engine-death suite asks for.
        /// </remarks>
        internal static void Configure(
            bool enabled,
            int stateBudget,
            int timeBudgetMs,
            int menuTimeBudgetMs,
            int memoryBudgetMb,
            LookAheadDiagnosticsWriter? diagnostics,
            int recoveryLimit = -1)
        {
            _diagnostics?.Flush();

            _enabled = enabled;
            _stateBudget = stateBudget;
            _timeBudgetMs = timeBudgetMs;
            _menuTimeBudgetMs = menuTimeBudgetMs;
            _memoryBudgetMb = memoryBudgetMb;
            _diagnostics = diagnostics != null && diagnostics.Enabled ? diagnostics : null;

            // A NEW POLICY RATHER THAN A SETTING ON THE OLD ONE, so a suite that changes the
            // limit also starts from an empty stash and an unspent budget. Changing the
            // number underneath a policy that had already convicted something would carry
            // the last suite's evidence into this one.
            //
            // EVERY TIME, EVEN WHEN THE LIMIT IS NOT NAMED, which is de-pszk's second half.
            // The count leaks the same way the engine does: a suite that spends two of the
            // five deaths leaves the next suite three, and the next suite is measuring a
            // policy it never asked for. A caller that says nothing about the limit is
            // asking for the shipped one, not for whatever the last suite left behind.
            _recovery = recoveryLimit >= 0
                ? new EngineRecovery(recoveryLimit)
                : new EngineRecovery();

            // The budgets are not applied to an engine here any more; they travel in the
            // request, and the engine on the other side of the bridge applies them. See
            // LookAheadRequest.MemoryBudgetMb and TimeBudgetMs.
        }

        /// <summary>
        /// Asks the bridge about every option of a menu at once, before any of them is
        /// drawn.
        /// </summary>
        /// <remarks>
        /// <para>ONE CALL PER MENU, not one per option. <c>OnConversationResponseMenu</c> is
        /// where the whole list exists - it is the loop that calls
        /// <c>ChooseResponseText</c> for each - so this is the only place the batching can
        /// happen at all.</para>
        ///
        /// <para>Grouped by conversation, which is almost always one group and one call: a
        /// link can leave its conversation, and the engine loads a group from the
        /// conversation it is given, so options that come from somewhere else have to be
        /// asked about separately or be answered as though nothing were reachable.</para>
        ///
        /// <para>NOTHING HERE MAY THROW INTO THE GAME. A menu the bridge could not be asked
        /// about is a menu with no bridge answers to compare, which the comparison counts
        /// and reports.</para>
        /// </remarks>
        private static void PrepareMenu(Il2CppReferenceArray<Response> responses)
        {
            _menuAnswers = null;

            GlobalStateSession? session = _session;
            LookAheadIndex? bridge = Bridge();
            if (bridge == null || session == null || responses == null)
            {
                return;
            }

            var byConversation = new Dictionary<int, List<DialogueNodeId>>();
            foreach (Response response in responses)
            {
                DialogueEntry? entry = response?.destinationEntry;
                if (entry == null)
                {
                    continue;
                }

                if (!byConversation.TryGetValue(entry.conversationID, out List<DialogueNodeId>? starts))
                {
                    starts = new List<DialogueNodeId>();
                    byConversation[entry.conversationID] = starts;
                }

                starts.Add(new DialogueNodeId(entry.conversationID, entry.id));
            }

            foreach (KeyValuePair<int, List<DialogueNodeId>> group in byConversation)
            {
                AskAbout(bridge, session, group.Key, group.Value);
            }
        }

        /// <summary>Asks one conversation group about the options that start in it.</summary>
        private static void AskAbout(
            LookAheadIndex bridge,
            GlobalStateSession session,
            int conversation,
            List<DialogueNodeId> starts)
        {
            // CONVICTED OF KILLING ENGINES, so it is not asked again this session. The
            // options still get a marker - the uncertain one, from MarkerFor - because a
            // search really did run here and really did not finish.
            if (_recovery.IsQuarantined(conversation))
            {
                return;
            }

            long began = System.Diagnostics.Stopwatch.GetTimestamp();
            try
            {
                // A rebuild replaced the index, so anything cached against the old one is
                // about a file that no longer exists. The questions matter especially: the
                // world is answered BY POSITION against them, so a stale list would put
                // every answer on the wrong question rather than simply being out of date.
                if (_questionsGeneration != bridge.Generation)
                {
                    _questions.Clear();
                    _questionsGeneration = bridge.Generation;
                }

                if (!_questions.TryGetValue(conversation, out LookAheadQuestions? questions))
                {
                    questions = bridge.Engine.QuestionsFor(conversation);

                    // The index is a cache of the dialogue database; this is where it is
                    // checked, on first use of the group, and where a rebuild happens if it
                    // turns out to describe a different game. A rebuild replaces the engine,
                    // so the questions are asked again afterwards.
                    if (!bridge.IsValidFor(questions.Conversations))
                    {
                        return;
                    }

                    // Asked again, because a rebuild replaced the engine underneath the
                    // first answer.
                    _questionsGeneration = bridge.Generation;
                    questions = bridge.Engine.QuestionsFor(conversation);
                    _questions[conversation] = questions;
                }

                LookAheadRequest request =
                    LookAheadRequestBuilder.Build(conversation, questions, session);

                // THE BUDGETS THE PLAYER SET, sent rather than applied here. The crawl is
                // on the other side of the bridge, so a budget that stays in this process
                // limits nothing - which is exactly what happened when the marker was
                // flipped over and one of these was left out: a budget of one still marked
                // three options, because the engine never heard about it.
                request.StateBudget = _stateBudget;
                request.TimeBudgetMs = _timeBudgetMs;
                request.MenuTimeBudgetMs = _menuTimeBudgetMs;
                request.MemoryBudgetMb = _memoryBudgetMb;
                foreach (DialogueNodeId start in starts)
                {
                    request.Starts.Add(new NodeRef(start.ConversationId, start.EntryId));
                }

                // WHERE THE PLAYER HAS BEEN, as recorded. The engine works out what it cuts, the
                // same way it does for an offline walk.
                foreach (NodeRef entry in _walk.Shown)
                {
                    request.Encountered.Add(entry);
                }

                // BEFORE IT IS ASKED, as the message that crosses rendered to text. What
                // this is for is the comparison nothing could make until now: an offline
                // run assembles the same world out of the committed save and the staged
                // state, and when the two disagree about a menu the question is which
                // FIELD differs.
                //
                // RENDERED RATHER THAN THE BYTES THEMSELVES, because what crosses is now
                // binary and a person diffing two of these needs to read them. It is the
                // same message either way - the rendering comes from the generated code,
                // so it cannot describe a field the wire does not carry - and it is built
                // only when the diagnostics are on, since the call short-circuits.
                _diagnostics?.RecordRequest(conversation, WireConvert.Write(request).ToString());

                LookAheadResponse answered = bridge.Engine.Ask(request);
                if (answered.Error != null)
                {
                    _log?.Warning(
                        $"{LogPrefix} conversation {conversation} was "
                        + $"refused: {answered.Error}");
                    return;
                }

                _menuAnswers = answered;

                // IT ANSWERED, SO IT IS NOT THE KILLER. A group that killed an engine and
                // then answered in the fresh one has shown the fault was cumulative rather
                // than its own, and must not be convicted by whatever dies next.
                _recovery.RecordAnswered(conversation);

                // RECORDED HERE RATHER THAN WHERE AN OPTION IS DRAWN, and it is the first
                // time a rolled check's crawls are recorded at all. They used to reach the
                // diagnostics through MarkerFor, as the one combined answer a check had -
                // and that answer reported ZERO states and zero entries, because the pair
                // it was derived from carried no cost figures. Every outcome is an ordinary
                // answer now, with its own, so recording the response is recording the
                // truth.
                if (_diagnostics != null)
                {
                    foreach (LookAheadAnswer recorded in answered.Answers)
                    {
                        _diagnostics.Record(
                            recorded, _memoryBudgetMb, questions.Entries.Count, request.World);
                    }
                }
            }
            catch (EngineDiedException died)
            {
                // NOT A FAILED CALL. The engine itself has gone, so there is nothing this
                // menu can be told - but there may well be a next engine, and the group
                // that was in flight is what the decision about one turns on.
                EngineDied(died, conversation);
            }
            catch (Exception error)
            {
                // The whole point of running both engines is that this one is not yet
                // trusted. A failure costs the comparison for this menu and nothing else.
                _log?.Warning(
                    $"{LogPrefix} conversation {conversation} could not be "
                    + $"asked ({error.GetType().Name}: {error.Message}).");
            }
        }

        /// <summary>
        /// Starts opening the engine, off the game's threads. Called once, at load.
        /// </summary>
        /// <remarks>
        /// See <see cref="_warming"/> for why this is not done here and now. A second call
        /// is ignored rather than starting a second engine, which matters because
        /// <see cref="Install"/> is called again by the in-game suites.
        /// </remarks>
        private static void BeginOpening()
        {
            if (_warming != null || _bridgeOpened)
            {
                return;
            }

            IGlobalStateLog? log = _log;
            string? modDirectory = _modDirectory;
            if (log == null || modDirectory == null)
            {
                return;
            }

            // Read here rather than inside the task, so the task borrows nothing that a
            // later Install could change underneath it.
            string pluginDirectory = NativeEngineCheck.PluginDirectory;
            _warming = Task.Run(() => Opened(pluginDirectory, modDirectory, log));
        }

        /// <summary>Opens the bridge, turning any failure into null and a log line.</summary>
        /// <remarks>
        /// NOTHING MAY ESCAPE THIS. It runs on a thread nobody is waiting on, and an
        /// exception there would be an unobserved task fault rather than anything a player
        /// or a log reader would ever see.
        /// </remarks>
        private static LookAheadIndex? Opened(
            string pluginDirectory, string modDirectory, IGlobalStateLog log)
        {
            try
            {
                return LookAheadIndex.Open(pluginDirectory, modDirectory, log);
            }
            catch (Exception error)
            {
                // A missing or unrunnable engine arrives here from the attempt to start it,
                // rather than from anything this file does.
                log.Warning(
                    $"{LogPrefix} the look-ahead engine is unavailable "
                    + $"({error.GetType().Name}: {error.Message}). "
                    + "The managed engine is answering on its own.");
                return null;
            }
        }

        /// <summary>
        /// Kills the engine process, for a harness that wants to see what happens next.
        /// </summary>
        /// <remarks>
        /// <para>TEST-ONLY, and reached from <c>KillLookAheadEngine</c>. Nothing in play
        /// calls it. It kills rather than closes because the case worth provoking is an
        /// engine that died on its own; a clean shutdown is the one that already works.</para>
        ///
        /// <para>It does NOT report the death itself. The mod is supposed to find that out
        /// the way it would in earnest - by asking the engine something and getting nothing
        /// back - and a shutdown triggered from here would prove that this method works
        /// rather than that the detection does.</para>
        ///
        /// <para>Waits for the process to be gone before returning, so a suite that kills
        /// and then opens a menu cannot race a child that is still on its way out.</para>
        /// </remarks>
        /// <summary>
        /// The engine process the mod is currently using, or 0 if it has none yet.
        /// </summary>
        /// <remarks>
        /// <para>TEST-ONLY, and the read-only counterpart of
        /// <see cref="KillEngineForTesting"/>. A suite that kills an engine and then wants
        /// to see the REPLACEMENT arrive needs to know when it has, and the alternative is
        /// sleeping for a guess: the replacement costs a process launch plus a 173-244 ms
        /// index read, which is fast enough that a fixed wait is either flaky or wasteful.
        /// Comparing the id against the one that was killed says exactly when it is
        /// there.</para>
        ///
        /// <para>IT GOES THROUGH <see cref="Bridge"/> RATHER THAN READING THE FIELD, which
        /// is the whole reason this works: the replacement lands in a Task, and Bridge is
        /// what promotes a finished one to the live bridge. Reading _bridge directly would
        /// report 0 for ever, because nothing else would ever collect the result until the
        /// next response menu.</para>
        ///
        /// <para>On the game's own thread, like every probe command, which is the thread
        /// Bridge is otherwise called from.</para>
        /// </remarks>
        /// <returns>The engine's process id, or 0 if there is no engine right now.</returns>
        internal static int EngineProcessForTesting() => Bridge()?.Engine.ProcessId ?? 0;

        /// <returns>The process id that was killed, or 0 if there was no engine.</returns>
        internal static int KillEngineForTesting()
        {
            int id = _bridge?.Engine.ProcessId ?? 0;
            if (id == 0)
            {
                return 0;
            }

            using System.Diagnostics.Process engine =
                System.Diagnostics.Process.GetProcessById(id);
            engine.Kill();
            engine.WaitForExit();

            _log?.Warning(
                $"{LogPrefix} the look-ahead engine (process {id}) was killed on purpose "
                + "by a harness. What happens next is the thing being tested.");
            return id;
        }

        /// <summary>
        /// Gives the next suite an engine, when the last one killed the only engine there
        /// was and forbade a replacement.
        /// </summary>
        /// <remarks>
        /// <para>de-pszk. TEST-ONLY, and reached from <c>PrepareLookAheadSuite</c>. The game
        /// is ONE PROCESS for a whole run, so the engine-death suite - which sets the
        /// recovery limit to zero precisely so that one kill reaches the give-up notice -
        /// leaves every suite after it with no engine and no way to get one. Running
        /// engine-death before engine-recovery failed on exactly that: the second suite's
        /// first kill found nothing to kill.</para>
        ///
        /// <para>IT BELONGS AT SUITE PREPARE, beside the diagnostics flush and the
        /// harness's clearing of artefacts, and for the same reason: a suite must not be
        /// satisfied - or, here, defeated - by what its predecessor left behind.</para>
        ///
        /// <para>NOTHING TO DO IN THE ORDINARY CASE. A suite that ends with a live engine,
        /// or with a replacement already on its way, is left exactly as it is. Only the
        /// give-up, which is final for a session by design, has to be undone.</para>
        ///
        /// <para>THE ENGINE COMES UP BEHIND THIS, not inside it: starting it is
        /// <see cref="BeginOpening"/>, the same as at load, and a process launch plus a
        /// fourteen-megabyte index read is not something to hold a probe command open for.
        /// A harness about to kill an engine therefore has to ASK for the process id until
        /// it has one rather than assume it is already there.</para>
        /// </remarks>
        internal static void ReviveForSuite()
        {
            if (!_engineDied || _harmonyId == null)
            {
                return;
            }

            _log?.Warning(
                $"{LogPrefix} the look-ahead had given up for this session and a new suite "
                + "is starting, so the hooks go back on and a fresh engine is started. "
                + "TEST-ONLY: nothing in play asks for this.");

            _engineDied = false;

            // NOT A RESPAWN. _respawning is what draws the uncertain marker on menus during
            // the wait and what makes a failed open spend the recovery budget, and neither
            // is right here: nothing died on this suite's watch, and it has asked nothing
            // yet.
            _respawning = false;

            // BOTH, and in this order, because BeginOpening refuses to start a second
            // engine while either says one is already there. GiveUpOnLookAhead left
            // _bridgeOpened true over a null bridge, which is the state that means "there
            // is no engine and there never will be".
            _bridgeOpened = false;
            _warming = null;
            BeginOpening();

            // The hooks came off with the give-up, and a look-ahead nothing calls into
            // would draw every option unmarked however healthy its engine was.
            PatchTheHooks();
        }

        /// <summary>
        /// The engine has gone: decide whether to replace it, and act on the decision.
        /// </summary>
        /// <remarks>
        /// <para>de-bnjy.1.3 TURNED THIS FROM A SHUTDOWN INTO A DECISION. It used to stop
        /// the feature for the session on the first death, on the reasoning that an engine
        /// which failed on a menu will fail on it again. That reasoning turned out to fit
        /// the wrong fault: the only death ever observed here (de-fpax) was CUMULATIVE -
        /// conversation 28's row overflowed the stack as the third row of a run and
        /// finished in 58 ms in a process of its own - and a fresh process is exactly what
        /// resets an accumulation. So most deaths are now answered by a new engine and no
        /// message at all.</para>
        ///
        /// <para><see cref="EngineRecovery"/> holds the decision and the reasoning behind
        /// it, deliberately apart from this method so it can be tested without a game.
        /// What is here is only the acting on it.</para>
        /// </remarks>
        /// <param name="died">How the engine went, which the player is told about if this
        /// is the last of them.</param>
        /// <param name="conversation">The group whose request was in flight, or null where
        /// there was none to blame.</param>
        private static void EngineDied(EngineDiedException died, int? conversation)
        {
            if (_engineDied)
            {
                return;
            }

            RecoveryAction action = _recovery.RecordDeath(conversation);
            if (action == RecoveryAction.QuarantineAndRespawn && conversation.HasValue)
            {
                // SAID OUT LOUD, because from here on that group's options are drawn
                // uncertain for no reason a log reader could otherwise see.
                _log?.Warning(
                    $"{LogPrefix} conversation {conversation.Value} has now killed two "
                    + "engines, the second of them freshly started, so the fault is the "
                    + "group's rather than something left behind by earlier searches. It "
                    + "will not be asked about again this session; its options will be "
                    + "drawn with the uncertain marker.");
            }

            if (action != RecoveryAction.GiveUp)
            {
                Respawn(died);
                return;
            }

            GiveUpOnLookAhead(died);
        }

        /// <summary>
        /// The end of the road: say so once, take the hooks off, and stop for good.
        /// </summary>
        /// <remarks>
        /// <para>WHAT THIS WHOLE FILE USED TO DO ON THE FIRST DEATH, and now does only when
        /// <see cref="EngineRecovery"/> has run out of budget - either because too many
        /// engines died or because a replacement could not be started at all. Split out
        /// from <see cref="EngineDied"/> so that <see cref="StartupFailed"/> can reach it
        /// without pretending an engine died, which would spend the budget a second time
        /// for the same failure.</para>
        ///
        /// <para>THE HOOKS COME OFF rather than a flag being checked in them. Harmony can
        /// genuinely undo itself, and unpatching by this class's OWN Harmony id takes off
        /// exactly the two look-ahead patches - which is why they were given an instance of
        /// their own. Everything else the plugin does keeps running: the tracking hooks
        /// above all, because losing what the player has read because a search process died
        /// would turn a cosmetic failure into data loss.</para>
        ///
        /// <para>Once. Every path in reaches this, and a message repeated on every response
        /// menu after the engine has gone would be worse than the silence it replaced.</para>
        /// </remarks>
        private static void GiveUpOnLookAhead(EngineDiedException died)
        {
            if (_engineDied)
            {
                return;
            }

            _engineDied = true;

            string advice = Advice(died.Death);
            string recovered = Recoveries();
            _log?.Warning(
                $"{LogPrefix} {EngineHasGone}. "
                + $"{advice}{recovered} Dialogue options will be drawn without look-ahead "
                + $"markers for the rest of this session; {RestartAdvice}. "
                + "Tracking, the counts and the option colours are unaffected. "
                + $"({died.Death}: {died.Message})");

            // ON SCREEN AS WELL AS IN THE LOG, because nobody plays with the log open.
            // TWO TEXTS FOR THE TWO CHANNELS, because they are shaped completely
            // differently: the window wraps, and waits for the player, so it can afford to
            // name itself and say what was lost; the notification is one unwrapped line
            // that passes by in a couple of seconds, so anything past a sentence would be
            // clipped by the edge of the screen before it could be read.
            TellThePlayer(
                WindowNotice(died.Death, recovered),
                $"Look-ahead markers are off. {advice} Please {RestartAdvice}.");

            // The bridge first, so nothing is left holding a dead engine, and then the
            // hooks, so nothing calls in again while this is happening.
            try
            {
                _bridge?.Dispose();
            }
            catch (Exception)
            {
                // It is already dead; disposing it is tidiness, not a step that can fail
                // in a way anybody can act on.
            }

            _bridge = null;
            _bridgeOpened = true;
            _menuAnswers = null;
            _questions.Clear();

            try
            {
                // UnpatchSelf, not UnpatchAll(id): the id overload is obsolete in the
                // Harmony this builds against, and UnpatchAll now means EVERYTHING - which
                // would take the tracking hooks with it, the one outcome this whole
                // arrangement exists to prevent. The compiler says so; note that the
                // published API reference does not.
                _harmony?.UnpatchSelf();
            }
            catch (Exception error)
            {
                // A hook that will not come off is not a reason to stop: _engineDied is
                // already set, so the patches that remain do nothing anyway. Said out loud
                // because it would otherwise be invisible.
                _log?.Warning(
                    $"{LogPrefix} the look-ahead hooks could not be removed "
                    + $"({error.GetType().Name}: {error.Message}); they will do nothing.");
            }

            _harmony = null;
        }

        /// <summary>Throws the dead engine away and starts another, off this frame.</summary>
        /// <remarks>
        /// <para>NO NEW MACHINERY, which is the whole reason this is cheap enough to do.
        /// <see cref="Bridge"/> already returns null while <see cref="_warming"/> is
        /// incomplete, and a null bridge already means "draw this menu without markers" -
        /// so clearing the flags and starting a fresh warm-up gets exactly the behaviour
        /// de-bnjy.1.3 asks for: the menu in front of the player draws unmarked, now, and
        /// the new engine comes up behind it.</para>
        ///
        /// <para>THAT IT IS OFF THIS FRAME IS THE POINT, and the cost it stays off is
        /// measured rather than assumed: reading the 14.4 MB index takes 173 to 244 ms
        /// (repeat_question, 2026-09-07; performance/removed_tools.md says how to price it
        /// again) on top of the process launch, and
        /// since de-2wtl the warm diagram manager for the group the player is standing in
        /// goes with the old process too - another sixty-odd milliseconds on the next menu
        /// (crates/gct-measure/examples/workspace_menus.rs). None of that may happen while a response menu
        /// is being drawn.</para>
        ///
        /// <para>THE HOOKS STAY ON, unlike the give-up path. They are what will notice the
        /// new engine when it arrives; taking them off would make the respawn pointless.</para>
        /// </remarks>
        private static void Respawn(EngineDiedException died)
        {
            _log?.Warning(
                $"{LogPrefix} {EngineReplaced}; this menu, and any drawn before it "
                + "arrives, will have no look-ahead markers. "
                + $"({died.Death}: {died.Message})");

            try
            {
                _bridge?.Dispose();
            }
            catch (Exception)
            {
                // It is already dead; disposing it is tidiness, not a step that can fail
                // in a way anybody can act on.
            }

            // IN PASSING, NOT IN A WINDOW. The window stops the game and is reserved for
            // the one fatal message - "one fatal, once, or it becomes the thing players mod
            // out", per TellThePlayer - and this is the opposite of fatal: the feature is
            // coming back by itself in a moment. A crash the player can see explains the
            // '*?' that is about to appear on every option, which silence would not.
            TellThePlayerInPassing(RecoveryLine);

            _bridge = null;
            _menuAnswers = null;

            // KEYED BY CONVERSATION AND VALIDATED BY GENERATION, so questions cached
            // against the dead engine describe an object that no longer exists.
            _questions.Clear();

            // BOTH, and in this order, because BeginOpening refuses to start a second
            // engine while either says one is already there.
            _bridgeOpened = false;
            _warming = null;

            _respawning = true;
            BeginOpening();
        }

        /// <summary>What to say about the engines that were replaced without a word.</summary>
        /// <remarks>
        /// THE CAUTION de-bnjy.1.3 RAISED: silence through several recoveries and then a
        /// sudden shutdown message reads as a fresh and unrelated failure. So the message
        /// that does end the feature accounts for the ones the player never saw, which is
        /// the only place that history is visible at all.
        /// </remarks>
        private static string Recoveries()
        {
            int replaced = _recovery.Respawns;
            if (replaced == 0)
            {
                return string.Empty;
            }

            string crashes = replaced == 1
                ? "An earlier crash was"
                : $"{replaced} earlier crashes were";
            return $" {crashes} recovered from without interrupting play.";
        }

        /// <summary>Who is talking, and what has happened to it.</summary>
        /// <remarks>
        /// NAMED, unlike the notification, which had no room to. A window that appears over
        /// a conversation saying a "look-ahead engine" has crashed, in a game that ships
        /// nothing of the kind, would send a player looking for the fault in the game.
        /// </remarks>
        private const string NoticeHeading =
            "Global Conversation Tracker Plugin:\nLook-Ahead Engine Crash";

        /// <summary>What happened, in terms of what the thing was for.</summary>
        private const string NoticeWhatHappened =
            "The process responsible for determining if dialogue options can potentially "
            + "lead to unseen dialogue crashed.";

        /// <summary>The one death with an answer of its own.</summary>
        /// <remarks>
        /// Names the setting, because it is the only thing the player can change that
        /// changes the outcome - and a budget that ran out once will run out again on the
        /// same conversation after the restart, so the restart alone is not the whole
        /// advice here. Said in the log and in the window from this one constant.
        /// </remarks>
        private const string OutOfMemoryAdvice =
            "It ran out of memory. Lowering LookAheadMemoryBudgetMb may help.";

        /// <summary>Every other death, which has no answer beyond the restart.</summary>
        private const string StoppedAdvice = "It stopped unexpectedly.";

        /// <summary>What is lost, in the terms the player sees it in.</summary>
        /// <remarks>
        /// THE MARKERS ARE DESCRIBED, not named: a player who never read the mod's
        /// documentation knows the asterisks by sight and by nothing else. The colours are
        /// the defaults - both are configurable, and a copy that named a colour the player
        /// had changed would be worse than one that named none.
        /// </remarks>
        private const string NoticeWhatItCosts =
            "As a result, the look-ahead dialogue markers (red or orange '*' characters at "
            + "the end of dialogue options) will no longer be drawn.";

        /// <summary>The one thing the player can do about it, at length.</summary>
        private const string NoticeWhatToDo =
            "Restart game to re-enable dialogue look-ahead.";

        /// <summary>Builds the window's text for the way the engine died.</summary>
        /// <remarks>
        /// Blank lines between the parts rather than one paragraph: the window centres
        /// what it is given, and four centred sentences run together read as a wall.
        /// </remarks>
        private static string WindowNotice(EngineDeath death, string recovered)
        {
            // Only the memory death adds a line. "It stopped unexpectedly" is what the
            // heading already says, in a word.
            string happened = death == EngineDeath.OutOfMemory
                ? $"{NoticeWhatHappened}\n{OutOfMemoryAdvice}"
                : NoticeWhatHappened;

            // The recoveries go with what happened rather than in a paragraph of their
            // own: they are the rest of the sentence "the process crashed", not a separate
            // piece of news. TrimStart because Recoveries leads with the space that joins
            // it to the log line, and a line of a centred window must not start with one.
            if (recovered.Length > 0)
            {
                happened += $"\n{recovered.TrimStart()}";
            }

            return string.Join(
                "\n\n", NoticeHeading, happened, NoticeWhatItCosts, NoticeWhatToDo);
        }

        /// <summary>What the log says about how the engine died.</summary>
        private static string Advice(EngineDeath death)
        {
            return death == EngineDeath.OutOfMemory ? OutOfMemoryAdvice : StoppedAdvice;
        }

        /// <summary>
        /// Puts the notice in front of the player, in the game's own confirmation window.
        /// </summary>
        /// <remarks>
        /// <para>THE GAME'S OWN CHANNEL RATHER THAN A ROW OF OUR OWN, and the deciding
        /// argument is WHEN this fires. The engine dies while a response menu is being
        /// drawn, which is to say in the middle of a conversation - and the HUD counts are
        /// children of the money display precisely so they FADE OUT during dialogue. A
        /// notice placed beside them would therefore be invisible at the only moment it is
        /// worth anything, and would surface later, over some unrelated scene, as a warning
        /// about something the player had already stopped noticing.</para>
        ///
        /// <para>A WINDOW RATHER THAN A NOTIFICATION, which is what this used to raise and
        /// is what <see cref="ShowTheNotice"/> still raises when there is no window to be
        /// had. A photograph of the notification (de-gbl3, on the killed menu at Siileng)
        /// settled it: the game draws that channel as a single unwrapped line across the
        /// foot of the screen, so at 1280 wide the message lost a word off each end, and
        /// <c>Failure</c> - the honest type for a feature that failed - brings the dice and
        /// the words CHECK FAILURE with it, which over a response menu reads as a roll the
        /// player just lost rather than as a mod that has stopped. Both faults are the
        /// channel's rather than the message's, and neither survives moving channel.</para>
        ///
        /// <para><c>ConfirmationController</c> is the window the game asks its own
        /// questions in - overwrite this save, quit to menu, keep this resolution. It
        /// wraps its text in a panel, it lives in the Init scene so it exists for as long
        /// as the game does, and its canvas overrides sorting at 1050, above everything
        /// the conversation draws. It is a bigger interruption than a notification, which
        /// is the point: the feature is gone until the game is restarted, and that is
        /// worth stopping for. It is also the reason nothing else in this mod may ever use
        /// it - one fatal, once, or it becomes the thing players mod out.</para>
        ///
        /// <para>IT MUST NOT THROW. This runs inside the handler for the engine having
        /// died; a notice that took the game down would be a worse failure than the one it
        /// is reporting, and the whole failure budget of this feature is "the asterisk does
        /// not appear". Every way it can go wrong ends in a log line and nothing else - the
        /// full message is already in the log by the time this is called - and every way
        /// the WINDOW can go wrong ends in the notification instead, which is worse than a
        /// window and much better than silence.</para>
        /// </remarks>
        private static void TellThePlayer(string window, string line)
        {
            if (!CanDrawOnScreen())
            {
                return;
            }

            if (ShowTheWindow(window))
            {
                return;
            }

            ShowTheNotice(line, NoticeShown);
        }

        /// <summary>Says one line in passing, and never in the window.</summary>
        /// <remarks>
        /// FOR THE RECOVERABLE CRASH. The window is reserved for the message that ends the
        /// feature - see the remarks on <see cref="TellThePlayer"/> for why it may be used
        /// once and for nothing else - and a crash the mod is already fixing does not
        /// deserve a modal stop. The player needs enough to explain the '*?' on the menu in
        /// front of them, which one passing line is.
        /// </remarks>
        private static void TellThePlayerInPassing(string line)
        {
            if (!CanDrawOnScreen())
            {
                return;
            }

            ShowTheNotice(line, RecoveryNoticeShown);
        }

        /// <summary>Whether this thread may touch Unity objects, saying so if it may not.</summary>
        private static bool CanDrawOnScreen()
        {
            if (Thread.CurrentThread.ManagedThreadId == _mainThreadId)
            {
                return true;
            }

            // Not a crash and not silent. The log already carries the whole message.
            _log?.Warning(
                $"{LogPrefix} the on-screen notice was skipped because the engine's "
                + "death was noticed off the game's main thread, where Unity objects "
                + "cannot be touched. The line above is the whole of it.");
            return false;
        }

        /// <summary>The window's text as one log line.</summary>
        /// <remarks>
        /// The message is four paragraphs, and a log entry that spans lines cannot be
        /// grepped, counted, or read beside the entries around it - BepInEx prefixes the
        /// first line only, so the rest arrive looking like something else's output.
        /// </remarks>
        private static string OneLine(string message)
        {
            return message.Replace("\n", " / ", StringComparison.Ordinal);
        }

        /// <summary>
        /// The sound the game's own confirmations click with, and the bank it is in.
        /// </summary>
        /// <remarks>
        /// The defaults <c>ShowConfirmation</c> declares, passed rather than left off:
        /// Il2CppInterop generates the game's methods without their optional arguments, so
        /// there is nothing to leave off, and a silent window would be a quieter thing
        /// than any confirmation the game raises for itself.
        /// </remarks>
        private const string ConfirmSound = "small-switch-full";

        /// <summary>The bank <see cref="ConfirmSound"/> is in.</summary>
        private const string ConfirmSoundGroup = "gamestart";

        /// <summary>
        /// Raises the window, and says whether the player ended up looking at one.
        /// </summary>
        /// <remarks>
        /// <para>ONE BUTTON. <c>showCancel: false</c>, because there is nothing to cancel:
        /// the engine has already gone by the time this is called, and a window offering a
        /// choice about it would be offering a choice that does not exist.</para>
        ///
        /// <para>THE BUTTON CLOSES THE WINDOW ITSELF, rather than trusting that pressing it
        /// does. The game's bodies are stripped from every export we have, so which half of
        /// the pair - the button's own handler, or the action handed to it - is what
        /// actually closes the panel cannot be read anywhere; it can only be run. Of the two
        /// ways to be wrong, a window that closes twice is harmless and a window that never
        /// closes has swallowed the player's game, so the close is issued from our side and
        /// a second one, if the game issues its own, costs nothing.</para>
        /// </remarks>
        private static bool ShowTheWindow(string message)
        {
            try
            {
                if (!ConfirmationController.HasInstance)
                {
                    // Before the Init scene's UI exists. Rare, and not silent: the
                    // notification manager may still be there, and is tried next.
                    _log?.Warning(
                        $"{LogPrefix} there was no confirmation window to tell the player "
                        + "with; a notification will have to do.");
                    return false;
                }

                ConfirmationController window = ConfirmationController.Singleton;

                // Nullable because ConvertDelegate is: it answers null for a null
                // delegate, which is not what it was handed, and the compiler cannot know
                // that. The window's own parameters are unannotated, so it goes in as it
                // comes out.
                Il2CppSystem.Action? dismiss = DelegateSupport.ConvertDelegate<
                    Il2CppSystem.Action>(new Action(() => Dismiss(window)));

                window.ShowConfirmation(
                    message, dismiss, dismiss, false, ConfirmSound, ConfirmSoundGroup);
                _log?.Warning(
                    $"{LogPrefix} {NoticeShown} {NoticeInAWindow}: {OneLine(message)}");
                return true;
            }
            catch (Exception error)
            {
                _log?.Warning(
                    $"{LogPrefix} the confirmation window could not be opened "
                    + $"({error.GetType().Name}: {error.Message}); a notification will "
                    + "have to do.");
                return false;
            }
        }

        /// <summary>Closes the window, from the button that the player pressed.</summary>
        /// <remarks>
        /// Runs from the game's own UI event, back across the interop boundary, so it
        /// swallows what it catches for the same reason everything else here does: an
        /// exception thrown out of this lands in IL2CPP, which has nothing to catch it.
        /// </remarks>
        private static void Dismiss(ConfirmationController window)
        {
            try
            {
                window.OnCloseConfirmation(true);
            }
            catch (Exception error)
            {
                _log?.Warning(
                    $"{LogPrefix} the confirmation window would not close "
                    + $"({error.GetType().Name}: {error.Message}); the game's own button "
                    + "should still close it.");
            }
        }

        /// <summary>
        /// The fallback: the passing notification this notice used to be.
        /// </summary>
        /// <remarks>
        /// KEPT, rather than deleted along with the argument for it, because it is the
        /// only other channel that draws over a conversation. Its two faults - the line
        /// that does not wrap, the register that reads as a lost roll - both cost less
        /// than the player never finding out at all, which is what the alternative to a
        /// fallback is.
        /// </remarks>
        private static void ShowTheNotice(string message, string shown)
        {
            try
            {
                if (!NotificationSystem.NotificationManager.HasInstance)
                {
                    // Before the HUD exists - a death during the main menu, say. Nothing to
                    // draw on, and nothing the player is missing yet either.
                    _log?.Warning(
                        $"{LogPrefix} there was no notification manager to tell the player "
                        + "with either; the line above is the whole of it.");
                    return;
                }

                NotificationSystem.NotificationManager.Singleton.ShowNotification(
                    NotificationSystem.NotificationType.Failure, message);
                _log?.Warning(
                    $"{LogPrefix} {shown} {NoticeInANotification}: {message}");
            }
            catch (Exception error)
            {
                _log?.Warning(
                    $"{LogPrefix} the on-screen notice could not be shown "
                    + $"({error.GetType().Name}: {error.Message}); the line above is the "
                    + "whole of it.");
            }
        }

        /// <summary>The bridge, or null where there is none YET or at all.</summary>
        /// <remarks>
        /// The two nulls are deliberately not distinguished, because the caller does the
        /// same thing about both: the managed engine answers this menu. A menu drawn while
        /// the engine is still starting is therefore answered exactly as a menu is when
        /// there is no index deployed at all.
        /// </remarks>
        private static LookAheadIndex? Bridge()
        {
            if (_engineDied)
            {
                return null;
            }

            if (_bridgeOpened)
            {
                return _bridge;
            }

            Task<LookAheadIndex?>? warming = _warming;
            if (warming == null)
            {
                // Nothing started it, which means Install did not run or had nothing to
                // work with. Opening one here would put the whole cost in this frame.
                return null;
            }

            if (!warming.IsCompleted)
            {
                return null;
            }

            _bridgeOpened = true;
            // Faulted rather than returning null is not expected - Opened catches its own -
            // but an unobserved fault here would be a null reference several frames later.
            _bridge = warming.Status == TaskStatus.RanToCompletion ? warming.Result : null;

            if (_respawning)
            {
                _respawning = false;
                if (_bridge == null)
                {
                    // A REPLACEMENT THAT NEVER CAME UP, which spends from the same budget
                    // as a death. Otherwise a machine that can no longer start the engine
                    // at all would be asked to on every menu for the rest of the session
                    // and the give-up would never fire. Note that this counts ONLY for a
                    // respawn: an installation with no engine deployed fails the FIRST
                    // open every time and is supposed to carry on quietly, which is why
                    // _respawning exists.
                    StartupFailed();
                }
                else
                {
                    _log?.Info(
                        $"{LogPrefix} {EngineReplacementUp}; markers resume with this "
                        + "response menu.");
                }
            }

            return _bridge;
        }

        /// <summary>A replacement engine that could not be started at all.</summary>
        /// <remarks>
        /// Tries again until the budget is gone, and then says so the way any last death
        /// is said. The synthetic <see cref="EngineDiedException"/> is because the give-up
        /// message is written in terms of how an engine died and this one never lived; it
        /// carries <see cref="EngineDeath.Crashed"/>, whose advice is the restart, which is
        /// the right advice here too.
        /// </remarks>
        private static void StartupFailed()
        {
            if (_recovery.RecordStartupFailure() == RecoveryAction.GiveUp)
            {
                GiveUpOnLookAhead(
                    new EngineDiedException(
                        EngineDeath.Crashed,
                        "a replacement look-ahead engine could not be started"));
                return;
            }

            _log?.Warning(
                $"{LogPrefix} a replacement look-ahead engine could not be started; "
                + "another will be tried.");
            _bridgeOpened = false;
            _warming = null;
            _respawning = true;
            BeginOpening();
        }

        /// <summary>
        /// The time budget a millisecond setting asks for; zero or less means no limit.
        /// </summary>
        private static TimeSpan TimeBudgetOf(int milliseconds) =>
            milliseconds > 0 ? TimeSpan.FromMilliseconds(milliseconds) : TimeSpan.Zero;

        /// <summary>
        /// Says that a crawl is taking a noticeable amount of time, once a second.
        /// </summary>
        /// <remarks>
        /// Silent in play. Every crawl measured over the largest conversations in the
        /// game finished in well under the interval, and one that does not is stopped by
        /// the time budget shortly after saying so once. It exists for the runs that
        /// deliberately raise the limits, where the alternative to a line a second is a
        /// game that looks indistinguishable from a hung one.
        /// </remarks>
        private static void ReportProgress(
            DialogueNodeId start, int states, int nodes, TimeSpan elapsed)
        {
            _log?.Info(
                $"Look-ahead still searching from {start.ConversationId}:{start.EntryId} after "
                + $"{elapsed.TotalSeconds:N1}s: {states} states over {nodes} entries.");
        }

        /// <summary>Flushes diagnostics belonging to the current test suite.</summary>
        internal static void FlushDiagnostics()
        {
            _diagnostics?.Flush();
        }

        /// <summary>
        /// Refuses a colour Unity cannot read rather than falling back, for the same
        /// reason <see cref="NovelResponseColorPatch"/> does: a silent fallback is
        /// indistinguishable from the hook not working.
        /// </summary>
        private static string Validate(string html, string parameterName)
        {
            if (string.IsNullOrWhiteSpace(html))
            {
                throw new ArgumentException("The colour must not be empty.", parameterName);
            }

            if (!ColorUtility.TryParseHtmlString(html, out Color _))
            {
                throw new ArgumentException(
                    $"'{html}' is not a colour Unity can read. Use #RRGGBB, #RRGGBBAA, or a "
                    + "colour name such as 'orange'.",
                    parameterName);
            }

            return html;
        }

        /// <summary>
        /// The marker for one option, or null when it has earned none.
        /// </summary>
        /// <remarks>
        /// <para>The gate is "strictly better than what the option already shows". An option
        /// drawn as unseen-anywhere is already the strongest state there is, so it never
        /// gains a marker; one drawn as unseen-this-save gains only the orange kind; a
        /// spent option can gain either.</para>
        ///
        /// <para>THE ANSWER IS ALREADY IN HAND. Every option of this menu was asked about in
        /// one call before any of them was drawn - see <see cref="PrepareMenu"/> - so this
        /// is a dictionary lookup rather than a search. That is the whole reason the bridge
        /// takes a list of starts.</para>
        ///
        /// <para>NO ANSWER MEANS NO MARKER. The bridge could not be reached, the index would
        /// not open, or the option is not in the group that was asked about. A marker that
        /// says nothing is the honest reading of "nobody looked"; the alternative is one
        /// that says "nothing there" on the strength of a search that never ran.</para>
        /// </remarks>
        private static string? MarkerFor(DialogueEntry entry)
        {
            GlobalStateSession? session = _session;
            if (!_enabled || session == null || entry == null)
            {
                return null;
            }

            SeenState own = NoveltyOf(session, entry.conversationID, entry.id);
            if (own == SeenState.UnseenAnyGame)
            {
                // Already the most novel thing there is, so nothing can outrank it.
                return null;
            }

            // A CRASH BEING RECOVERED FROM IS UNCERTAIN, NOT EMPTY, and for the same
            // reason as the quarantine below: a search really did run against this menu and
            // really did not finish, which is exactly what the uncertain marker means
            // (de-pvq). Drawing nothing would say "there is nothing unread down there" on
            // the strength of a crash.
            //
            // EVERY OPTION, not the ones a crawl had got to. The engine died with the menu
            // half-answered at best, so nothing here is established either way.
            if (Recovering)
            {
                return Uncertain();
            }

            // A QUARANTINED GROUP IS UNCERTAIN, NOT EMPTY. It is the one place where "no
            // answer" must NOT mean "no marker": a search really did run against this
            // group, twice, and really did not finish, which is exactly what the uncertain
            // marker means (de-pvq). Drawing nothing here would say "there is nothing
            // unread down there" on the strength of two crashes.
            if (_recovery.IsQuarantined(entry.conversationID))
            {
                return Uncertain();
            }

            // THE ANSWER THAT NAMES NO OUTCOME, which is what an ordinary option gets.
            // A rolled check has none - it came back as two, one per outcome - so this
            // finds nothing for one and it draws no marker, which is what it already did:
            // the Pass / Fail line replaces the marker on a check, and a marker taken from
            // the better of two outcomes was a weaker restatement of it.
            var start = new NodeRef(entry.conversationID, entry.id);
            if (_menuAnswers?.Find(start, null) is not LookAheadAnswer answer)
            {
                return null;
            }

            if (answer.Best <= (int)own)
            {
                // NOT FINDING SOMETHING IS PROVISIONAL; FINDING IT IS NOT. The best is a
                // lower bound, so a search that ran out of budget has not established that
                // nothing is reachable - only that it did not get there. Drawing nothing
                // says the first, which is a claim the search did not make.
                return answer.Complete ? null : Uncertain();
            }

            // Above the option's own seen state, so something was actually reached. That is
            // definite even under a budget - a witness is a witness - so an incomplete
            // search that found one still draws the ordinary marker.
            string colour = answer.Best == (int)SeenState.UnseenAnyGame
                ? _unseenAnyGameHtml
                : _unseenThisGameHtml;
            return Draw(colour, FoundMarker);
        }

        /// <summary>The uncertain marker, or nothing where the player turned it off.</summary>
        /// <remarks>
        /// THREE PATHS DRAW IT and they mean the same thing each time - a search ran and did
        /// not establish an answer, so nothing is claimed either way (de-pvq). A crawl that
        /// ran out of budget, a group quarantined for killing engines, and a menu drawn
        /// while a crashed engine is being replaced. The setting that turns it off is the
        /// player's, and it has to turn all three off together.
        /// </remarks>
        private static string? Uncertain() =>
            _markUncertain ? Draw(_uncertainHtml, UncertainMarker) : null;

        /// <summary>One marker, in one colour, as the game's text markup.</summary>
        private static string Draw(string colourHtml, string marker) =>
            "<color=" + colourHtml + ">" + marker + "</color>";

        /// <summary>The colours as the player has configured them.</summary>
        private static MarkerPalette Palette() => new MarkerPalette(
            _unseenAnyGameHtml, _unseenThisGameHtml, _seenHtml, _uncertainHtml,
            _markUncertain);

        /// <summary>
        /// The Pass / Fail line for one option, or null where it has not earned one.
        /// </summary>
        /// <remarks>
        /// <para>The same lookup <see cref="MarkerFor"/> does, and for the same reason: the
        /// whole menu was asked about in one call before any of it was drawn, so this is a
        /// dictionary read rather than a search. The line itself is composed in
        /// <see cref="BranchLine"/>, which is pure text and therefore testable; this is only
        /// the part that needs the game.</para>
        ///
        /// <para>NO ANSWER MEANS NO LINE, exactly as it means no marker. A line drawn
        /// without one would be inventing two outcomes rather than reporting them.</para>
        /// </remarks>
        private static string? BranchLineFor(DialogueEntry entry)
        {
            if (!_enabled || _session == null || entry == null)
            {
                return null;
            }

            // NO PAIR MEANS NO LINE, exactly as an absent nested pair used to. An
            // ordinary option is answered once and names no outcome, so there is nothing
            // here to find - which is the same answer, arrived at from the shape of the
            // response rather than from a field inside one answer.
            Outcomes? outcomes = _menuAnswers?.OutcomesOf(
                new NodeRef(entry.conversationID, entry.id));

            return outcomes is Outcomes both
                ? BranchLine.For(both.Pass, both.Fail, Palette())
                : null;
        }

        /// <summary>Whether this option is one the mod drew a Pass / Fail line under.</summary>
        /// <remarks>
        /// The same lookup <see cref="BranchLineFor"/> makes, without composing the line -
        /// for a caller that needs to know the line is THERE rather than what it says. See
        /// <see cref="CheckBandInsetPatch"/>, which has to know which buttons carry one.
        /// </remarks>
        internal static bool HasBranchLine(DialogueEntry entry)
        {
            if (!_enabled || _session == null || entry == null)
            {
                return false;
            }

            return _menuAnswers?.OutcomesOf(new NodeRef(entry.conversationID, entry.id)) != null;
        }

        /// <summary>Wall time since a stopwatch timestamp, in milliseconds.</summary>
        private static double Milliseconds(long since)
        {
            return (System.Diagnostics.Stopwatch.GetTimestamp() - since) * 1000d
                / System.Diagnostics.Stopwatch.Frequency;
        }

        /// <summary>Writes out anything still buffered. Call at shutdown.</summary>
        internal static void Flush()
        {
            _diagnostics?.Flush();
        }

        /// <summary>
        /// How novel one entry is, from the two facts the mod already tracks: the game's
        /// own per-save SimStatus, and the global state's record of every other save.
        /// </summary>
        private static SeenState NoveltyOf(
            GlobalStateSession session, int conversationId, int entryId)
        {
            if (DialogueLua.GetSimStatus(conversationId, entryId) == "WasDisplayed")
            {
                return SeenState.SeenThisGame;
            }

            SimStatus global = session.EnsureInitialized().GetStatus(conversationId, entryId);
            return global == SimStatus.WasDisplayed
                ? SeenState.UnseenThisGame
                : SeenState.UnseenAnyGame;
        }

        /// <summary>
        /// The one place a whole response menu exists before any of it is drawn.
        /// </summary>
        /// <remarks>
        /// <c>OnConversationResponseMenu</c> is the loop that calls
        /// <c>ChooseResponseText</c> for each option, so a prefix here runs once per menu
        /// with every option in hand - which is what lets the bridge be asked one question
        /// instead of one per option.
        /// </remarks>
        [HarmonyPatch(
            typeof(Sunshine.ConversationLogger),
            nameof(Sunshine.ConversationLogger.OnConversationResponseMenu))]
        private static class ResponseMenuPatch
        {
            /// <summary>
            /// The parameter name is matched against the patched method by Harmony, so it
            /// has to stay <c>responses</c>.
            /// </summary>
            [HarmonyPrefix]
            private static void Prefix(Il2CppReferenceArray<Response> responses)
            {
                HookFailureLimiter? failures = _failures;
                if (failures == null || failures.HasGivenUp || !_enabled)
                {
                    return;
                }

                try
                {
                    PrepareMenu(responses);
                }
                catch (Exception ex)
                {
                    // Never fatal to the menu. Without bridge answers the managed engine
                    // draws exactly what it drew before any of this existed.
                    failures.Report(ex);
                }
            }
        }

        /// <summary>The one place every response's displayed text is composed.</summary>
        [HarmonyPatch(
            typeof(Sunshine.ConversationLogger),
            nameof(Sunshine.ConversationLogger.ChooseResponseText))]
        private static class ChooseResponseTextPatch
        {
            /// <summary>
            /// The parameter name is matched against the patched method by Harmony, so it
            /// has to stay <c>response</c>.
            /// </summary>
            [HarmonyPostfix]
            private static void Postfix(Response response, FinalResponseText __result)
            {
                HookFailureLimiter? failures = _failures;
                if (failures == null || failures.HasGivenUp || __result == null)
                {
                    return;
                }

                try
                {
                    if (response == null || response.destinationEntry == null)
                    {
                        return;
                    }

                    string? marker = MarkerFor(response.destinationEntry);
                    string? branches = BranchLineFor(response.destinationEntry);

                    // THE LINE REPLACES THE MARKER RATHER THAN JOINING IT. An option's
                    // marker is the better of what its two outcomes can reach, so on a
                    // rolled check it is a weaker restatement of what the line below
                    // already says - in the one place it cannot be read against the
                    // outcome it came from. Only a check ever gets a line, so this is
                    // exactly the set of options that lose their marker.
                    if (branches == null && marker != null)
                    {
                        __result.responseText += marker;
                    }

                    // AFTER the option's own marker, because this is a line BELOW the
                    // option and the marker belongs on the option's own line.
                    if (branches != null)
                    {
                        __result.responseText += branches;
                    }
                }
                catch (Exception ex)
                {
                    failures.Report(ex);
                }
            }
        }

        /// <summary>Adds an entry the conversation stepped through to the walk.</summary>
        /// <remarks>
        /// NOTHING HERE MAY THROW INTO THE GAME. An entry that cannot be read costs that
        /// entry's place in the walk, which at worst cuts less than a whole walk would.
        /// </remarks>
        private static void RecordEntry(DialogueEntry? entry)
        {
            HookFailureLimiter? failures = _failures;
            if (failures == null || failures.HasGivenUp)
            {
                return;
            }

            try
            {
                if (entry != null)
                {
                    _walk.Record(new NodeRef(entry.conversationID, entry.id));
                }
            }
            catch (Exception ex)
            {
                failures.Report(ex);
            }
        }

        /// <summary>
        /// THE WALK IS READ OFF THE GAME'S OWN TRAVERSAL, in the three places below, rather than
        /// off the lines it displays.
        /// </summary>
        /// <remarks>
        /// <para>WHAT A DISPLAYED LINE CANNOT SAY. A conversation steps through entries it never
        /// shows: a GROUP entry, which is what a hub is, and an entry whose condition failed onto
        /// a passthrough link - which is what a passive skill check becomes when it does not fire,
        /// since <c>PassiveNode.CheckSuccess</c> writes Passthrough onto the entry before
        /// answering. Recovering those from the links between two displayed lines works only
        /// while every step between them is a group entry, and stops dead at the first one that
        /// is not.</para>
        ///
        /// <para>MEASURED, in conversation 379: Rhetoric's check on entry 672 did not fire, so the
        /// game passed over it into strikehub and displayed neither. The hub never reached the
        /// walk, nothing was cut, and every option that led back out through that hub was
        /// recommended as leading onward.</para>
        ///
        /// <para>So the recording follows what the dialogue system itself walks.
        /// <c>EvaluateLinksAtPriority</c> is where an entry's links are followed: it expands a
        /// group entry in place and is where the recursion into a passthrough entry arrives, so
        /// it names every entry stepped over. <c>GetState</c> is hooked beside it for the entry
        /// the game is about to display, which is the one case links are not always evaluated
        /// for - a forced link composes the next state without them.</para>
        ///
        /// <para><c>EvaluateLinks</c> itself is NOT hooked. It calls straight into
        /// <c>EvaluateLinksAtPriority</c> for each condition priority, so everything it sees
        /// arrives there anyway; the only entry it would add is one already walked in the same
        /// evaluation, which is a repeat rather than a step.</para>
        ///
        /// <para>Each hook is a PREFIX, so entries arrive in the order they are walked rather
        /// than in the order the recursion unwinds, and <see cref="ConversationWalk.Record"/>
        /// drops the repeats the priority loop asks for.</para>
        /// </remarks>
        [HarmonyPatch(
            typeof(ConversationModel),
            "GetState",
            new Type[] { typeof(DialogueEntry), typeof(bool), typeof(bool), typeof(bool) })]
        private static class ConversationStatePatch
        {
            /// <summary>The parameter name has to stay <c>entry</c>.</summary>
            [HarmonyPrefix]
            private static void Prefix(DialogueEntry entry) => RecordEntry(entry);
        }

        /// <summary>
        /// An entry the game follows the links of: a group entry expanded in place, or a
        /// passthrough one recursed into.
        /// </summary>
        [HarmonyPatch(typeof(ConversationModel), "EvaluateLinksAtPriority")]
        private static class EvaluateLinksAtPriorityPatch
        {
            /// <summary>The parameter name has to stay <c>entry</c>.</summary>
            [HarmonyPrefix]
            private static void Prefix(DialogueEntry entry) => RecordEntry(entry);
        }

        /// <summary>A conversation starting, which starts its walk.</summary>
        [HarmonyPatch(
            typeof(Sunshine.ConversationLogger),
            nameof(Sunshine.ConversationLogger.OnConversationStart))]
        private static class ConversationStartPatch
        {
            [HarmonyPostfix]
            private static void Postfix() => _walk.Clear();
        }

        /// <summary>A conversation ending, after which nothing it showed bears on a menu.</summary>
        [HarmonyPatch(
            typeof(Sunshine.ConversationLogger),
            nameof(Sunshine.ConversationLogger.OnConversationEnd))]
        private static class ConversationEndPatch
        {
            [HarmonyPostfix]
            private static void Postfix() => _walk.Clear();
        }
    }
}
