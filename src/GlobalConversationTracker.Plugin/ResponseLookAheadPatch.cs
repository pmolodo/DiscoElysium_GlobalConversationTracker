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

        /// <summary>Whether the engine has died, which is final for this session.</summary>
        /// <remarks>
        /// Set once by <see cref="EngineDied"/> and never cleared. It is what makes the
        /// message appear once and the feature stay off - the hooks are removed as well,
        /// but a hook that is mid-call when the engine goes still has to find its way out.
        /// </remarks>
        private static bool _engineDied;

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
        /// <param name="session">The session novelty is read from.</param>
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
            Configure(enabled, stateBudget, timeBudgetMs, memoryBudgetMb, diagnostics);

            // Started now and finished elsewhere, so that neither the main menu nor the
            // first conversation waits for a process launch and a fifteen-megabyte parse.
            BeginOpening();

            // A SECOND INSTALL WOULD ORPHAN THE FIRST INSTANCE, and hooks nothing holds an
            // instance for cannot be taken off again. Load calls this once; a test may not.
            _harmony?.UnpatchSelf();
            _engineDied = false;

            // AN INSTANCE OF ITS OWN, whose id is the plugin's with a suffix. Unpatching is
            // by id, so hooks that share one cannot be removed separately - and these two
            // have to come off, when the engine dies, without taking the tracking hooks
            // with them.
            _harmony = new Harmony(harmony.Id + LookAheadPatchSuffix);

            // Two hooks, and they are not interchangeable. The menu one is where the whole
            // list of options exists, which is the only place a single bridge call can
            // cover all of them; the text one is where a marker can be attached to an
            // option's own string.
            _harmony.PatchAll(typeof(ResponseMenuPatch));
            _harmony.PatchAll(typeof(ChooseResponseTextPatch));
        }

        /// <summary>Changes suite-scoped behavior without reinstalling the hook.</summary>
        internal static void Configure(
            bool enabled,
            int stateBudget,
            int timeBudgetMs,
            int memoryBudgetMb,
            LookAheadDiagnosticsWriter? diagnostics)
        {
            _diagnostics?.Flush();

            _enabled = enabled;
            _stateBudget = stateBudget;
            _timeBudgetMs = timeBudgetMs;
            _memoryBudgetMb = memoryBudgetMb;
            _diagnostics = diagnostics != null && diagnostics.Enabled ? diagnostics : null;

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
                    GameWorldSnapshot.Build(conversation, questions, session);

                // THE BUDGETS THE PLAYER SET, sent rather than applied here. The crawl is
                // on the other side of the bridge, so a budget that stays in this process
                // limits nothing - which is exactly what happened when the marker was
                // flipped over and one of these was left out: a budget of one still marked
                // three options, because the engine never heard about it.
                request.StateBudget = _stateBudget;
                request.TimeBudgetMs = _timeBudgetMs;
                request.MemoryBudgetMb = _memoryBudgetMb;

                foreach (DialogueNodeId start in starts)
                {
                    request.Starts.Add(new NodeRef(start.ConversationId, start.EntryId));
                }

                LookAheadResponse answered = bridge.Engine.Ask(request);
                if (answered.Error != null)
                {
                    _log?.Warning(
                        $"{LogPrefix} conversation {conversation} was "
                        + $"refused: {answered.Error}");
                    return;
                }

                _menuAnswers = answered;

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
                // NOT A FAILED CALL. The engine itself has gone, so there is nothing to
                // retry and nothing to compare against for the rest of the session.
                EngineDied(died);
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
        /// The engine has gone: say so once, take the hooks off, and stop for good.
        /// </summary>
        /// <remarks>
        /// <para>NO RESTART, deliberately, and the user asked for it that way: an engine
        /// that failed on a given menu will fail on it again, so a respawn loop is a stutter
        /// rather than a recovery. What brings the feature back is restarting the game.</para>
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
        private static void EngineDied(EngineDiedException died)
        {
            if (_engineDied)
            {
                return;
            }

            _engineDied = true;

            string advice = Advice(died.Death);
            _log?.Warning(
                $"{LogPrefix} {EngineHasGone}. "
                + $"{advice} Dialogue options will be drawn without look-ahead markers for "
                + $"the rest of this session; {RestartAdvice}. "
                + "Tracking, the counts and the option colours are unaffected. "
                + $"({died.Death}: {died.Message})");

            // ON SCREEN AS WELL AS IN THE LOG, because nobody plays with the log open.
            // TWO TEXTS FOR THE TWO CHANNELS, because they are shaped completely
            // differently: the window wraps, and waits for the player, so it can afford to
            // name itself and say what was lost; the notification is one unwrapped line
            // that passes by in a couple of seconds, so anything past a sentence would be
            // clipped by the edge of the screen before it could be read.
            TellThePlayer(
                WindowNotice(died.Death),
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
        private static string WindowNotice(EngineDeath death)
        {
            // Only the memory death adds a line. "It stopped unexpectedly" is what the
            // heading already says, in a word.
            string happened = death == EngineDeath.OutOfMemory
                ? $"{NoticeWhatHappened}\n{OutOfMemoryAdvice}"
                : NoticeWhatHappened;

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
            if (Thread.CurrentThread.ManagedThreadId != _mainThreadId)
            {
                // Not a crash and not silent. The log already carries the whole message.
                _log?.Warning(
                    $"{LogPrefix} the on-screen notice was skipped because the engine's "
                    + "death was noticed off the game's main thread, where Unity objects "
                    + "cannot be touched. The line above is the whole of it.");
                return;
            }

            if (ShowTheWindow(window))
            {
                return;
            }

            ShowTheNotice(line);
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
        private static void ShowTheNotice(string message)
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
                    $"{LogPrefix} {NoticeShown} {NoticeInANotification}: {message}");
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
            return _bridge;
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

            Novelty own = NoveltyOf(session, entry.conversationID, entry.id);
            if (own == Novelty.UnseenAnyGame)
            {
                // Already the most novel thing there is, so nothing can outrank it.
                return null;
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
                return _markUncertain && !answer.Complete
                    ? Draw(_uncertainHtml, UncertainMarker)
                    : null;
            }

            // Above the option's own novelty, so something was actually reached. That is
            // definite even under a budget - a witness is a witness - so an incomplete
            // search that found one still draws the ordinary marker.
            string colour = answer.Best == (int)Novelty.UnseenAnyGame
                ? _unseenAnyGameHtml
                : _unseenThisGameHtml;
            return Draw(colour, FoundMarker);
        }

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
        private static Novelty NoveltyOf(
            GlobalStateSession session, int conversationId, int entryId)
        {
            if (DialogueLua.GetSimStatus(conversationId, entryId) == "WasDisplayed")
            {
                return Novelty.SeenThisGame;
            }

            SimStatus global = session.EnsureInitialized().GetStatus(conversationId, entryId);
            return global == SimStatus.WasDisplayed
                ? Novelty.UnseenThisGame
                : Novelty.UnseenAnyGame;
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
    }
}
