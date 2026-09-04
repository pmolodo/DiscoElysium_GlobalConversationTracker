// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using GlobalConversationTracker.Engine;
using GlobalConversationTracker.LookAhead;
using GlobalConversationTracker.Session;
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
        private static GlobalStateSession? _session;
        private static HookFailureLimiter? _failures;
        private static string _unseenAnyGameHtml = NovelResponseColorPatch.DefaultNovelColorHtml;
        private static string _unseenThisGameHtml = DefaultUnseenThisGameColorHtml;
        private static LookAheadEngine _engine = new LookAheadEngine();
        private static LookAheadEngine? _tracingEngine;
        private static LookAheadDiagnosticsWriter? _diagnostics;
        private static int _budget = new LookAheadOptions().StateBudget;
        private static int _timeBudgetMs;
        private static bool _enabled = true;
        private static IGlobalStateLog? _log;

        /// <summary>The native engine and the index it reads, or null if there is none.</summary>
        /// <remarks>
        /// Opened once, lazily, at the first response menu rather than at plugin load: the
        /// index is tens of megabytes and the cache check it enables needs a loaded
        /// dialogue database, neither of which belongs in the frame that draws the main
        /// menu. Null afterwards means the library or the index is not there, and the
        /// managed engine simply carries on.
        /// </remarks>
        private static LookAheadIndex? _bridge;
        private static bool _bridgeOpened;

        /// <summary>What the bridge said about the options of the menu being drawn.</summary>
        /// <remarks>
        /// Filled once per menu, read once per option. This is the whole reason the bridge
        /// takes a list of starts: the world is the same for every option drawn at once and
        /// it is the world that is expensive to send, so one call amortises the marshalling
        /// over the menu instead of paying it per option.
        /// </remarks>
        private static readonly Dictionary<DialogueNodeId, LookAheadAnswer> _menuAnswers =
            new Dictionary<DialogueNodeId, LookAheadAnswer>();

        /// <summary>What the engine asks about a group, cached because it cannot change.</summary>
        private static readonly Dictionary<int, LookAheadQuestions> _questions =
            new Dictionary<int, LookAheadQuestions>();

        private static BridgeComparison? _comparison;

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
        /// Applies the patch. Call once, from plugin load, after the session exists.
        /// </summary>
        /// <param name="harmony">The plugin's Harmony instance.</param>
        /// <param name="session">The session novelty is read from.</param>
        /// <param name="log">Where hook failures are reported.</param>
        /// <param name="modDirectory">
        /// Where the mod keeps its own files, so a stale index can be rebuilt into it.
        /// </param>
        /// <param name="unseenAnyGameHtml">Colour for reaching never-seen-anywhere text.</param>
        /// <param name="unseenThisGameHtml">Colour for reaching unseen-this-save text.</param>
        /// <param name="stateBudget">The most search states one option may cost.</param>
        /// <param name="timeBudgetMs">
        /// The longest one option's crawl may run for, in milliseconds; 0 for no limit.
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
            int stateBudget,
            int timeBudgetMs,
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
            _modDirectory = modDirectory
                ?? throw new ArgumentNullException(nameof(modDirectory));
            _failures = new HookFailureLimiter(
                "marking options that still lead somewhere unread", log);
            _unseenAnyGameHtml = Validate(unseenAnyGameHtml, nameof(unseenAnyGameHtml));
            _unseenThisGameHtml = Validate(unseenThisGameHtml, nameof(unseenThisGameHtml));
            Configure(enabled, stateBudget, timeBudgetMs, diagnostics);

            // Two hooks, and they are not interchangeable. The menu one is where the whole
            // list of options exists, which is the only place a single bridge call can
            // cover all of them; the text one is where a marker can be attached to an
            // option's own string.
            harmony.PatchAll(typeof(ResponseMenuPatch));
            harmony.PatchAll(typeof(ChooseResponseTextPatch));
        }

        /// <summary>Changes suite-scoped behavior without reinstalling the hook.</summary>
        internal static void Configure(
            bool enabled,
            int stateBudget,
            int timeBudgetMs,
            LookAheadDiagnosticsWriter? diagnostics)
        {
            _diagnostics?.Flush();

            // One comparison per suite, reported and then started again. Accumulating
            // across suites would make every summary include the last one's options, and
            // the summary is what a run asserts on.
            _comparison?.Report();
            _comparison = _log == null ? null : new BridgeComparison(_log);

            _enabled = enabled;
            _budget = stateBudget;
            _timeBudgetMs = timeBudgetMs;
            _diagnostics = diagnostics != null && diagnostics.Enabled ? diagnostics : null;

            TimeSpan time = TimeBudgetOf(timeBudgetMs);
            _engine = new LookAheadEngine(new LookAheadOptions
            {
                StateBudget = stateBudget,
                TimeBudget = time,
                OnProgress = ReportProgress,
            });
            _tracingEngine = _diagnostics != null && _diagnostics.RetriesOverflowsWithTrace
                ? new LookAheadEngine(new LookAheadOptions
                {
                    StateBudget = stateBudget,
                    // Deliberately untimed. The re-walk exists to explain an overflow that
                    // has already happened, and it is slower than the crawl it explains -
                    // it keeps a per-entry tally. Timing it would cut the explanation short
                    // exactly when the crawl was expensive enough to need one.
                    TimeBudget = TimeSpan.Zero,
                    CollectTrace = true,
                })
                : null;
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
        private static void PrepareMenu(Response[] responses)
        {
            _menuAnswers.Clear();

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
                foreach (DialogueNodeId start in starts)
                {
                    request.Starts.Add(new NodeRef(start.ConversationId, start.EntryId));
                }

                LookAheadResponse answered = bridge.Engine.Ask(request);
                if (answered.Error != null)
                {
                    _log?.Warning(
                        $"{BridgeComparison.LogPrefix} conversation {conversation} was "
                        + $"refused: {answered.Error}");
                    return;
                }

                foreach (LookAheadAnswer answer in answered.Answers)
                {
                    _menuAnswers[new DialogueNodeId(
                        answer.Start.Conversation, answer.Start.Entry)] = answer;
                }

                _comparison?.RecordMenu(Milliseconds(began));
            }
            catch (Exception error)
            {
                // The whole point of running both engines is that this one is not yet
                // trusted. A failure costs the comparison for this menu and nothing else.
                _log?.Warning(
                    $"{BridgeComparison.LogPrefix} conversation {conversation} could not be "
                    + $"asked ({error.GetType().Name}: {error.Message}).");
            }
        }

        /// <summary>The bridge, opened on first use, or null where there is none.</summary>
        private static LookAheadIndex? Bridge()
        {
            if (_bridgeOpened)
            {
                return _bridge;
            }

            _bridgeOpened = true;
            IGlobalStateLog? log = _log;
            if (log == null || _modDirectory == null)
            {
                return null;
            }

            try
            {
                _bridge = LookAheadIndex.Open(
                    NativeEngineCheck.PluginDirectory, _modDirectory, log);
            }
            catch (Exception error)
            {
                // A missing native library arrives here as a DllNotFoundException from the
                // first call rather than from anything this file does.
                log.Warning(
                    $"{BridgeComparison.LogPrefix} the native look-ahead is unavailable "
                    + $"({error.GetType().Name}: {error.Message}). "
                    + "The managed engine is answering on its own.");
                _bridge = null;
            }

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
            _comparison?.Report();
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
        /// The gate is "strictly better than what the option already shows". An option
        /// drawn as unseen-anywhere is already the strongest state there is, so it never
        /// gains a marker; one drawn as unseen-this-save gains only the orange kind; a
        /// spent option can gain either.
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
                // Already the most novel thing there is, so nothing can outrank it and no
                // crawl runs. Counted rather than compared: see BridgeComparison.NotCrawled.
                _comparison?.NotCrawled();
                return null;
            }

            DialogueDatabase database = DialogueManager.masterDatabase;
            LookAheadGraph? graph = LookAheadGraphBuilder.ForConversation(
                database, entry.conversationID);
            if (graph == null)
            {
                return null;
            }

            if (!LookAheadEngine.HasPotentialImprovement(
                graph, own, node => NoveltyOf(session, node.ConversationId, node.EntryId)))
            {
                // No entry in the graph outranks what this option already shows, so the
                // crawl is skipped and there is no best-reachable figure to compare.
                _comparison?.NotCrawled();
                return null;
            }

            var world = new GameLookAheadWorld();
            var start = new DialogueNodeId(entry.conversationID, entry.id);

            long ticks = System.Diagnostics.Stopwatch.GetTimestamp();
            LookAheadResult result = _engine.Evaluate(
                graph,
                start,
                world,
                node => NoveltyOf(session, node.ConversationId, node.EntryId));
            double managedMilliseconds = Milliseconds(ticks);

            if (_diagnostics != null)
            {
                _diagnostics.Record(
                    start, result, managedMilliseconds, _budget,
                    TraceOverflow(graph, start, world, session, result));
            }

            // BOTH ENGINES RAN; the managed one's answer is the one drawn. Comparing them
            // over real play is what turns "the crossing looks right" into evidence, and it
            // costs a dictionary lookup on top of a crawl that was happening anyway. The
            // marker switches to the bridge once the log goes quiet - see de-i5xj.8.
            _comparison?.Record(
                start,
                result.Best,
                managedMilliseconds,
                _menuAnswers.TryGetValue(start, out LookAheadAnswer answer)
                    ? answer
                    : (LookAheadAnswer?)null);

            if (result.Best <= own)
            {
                return null;
            }

            string colour = result.Best == Novelty.UnseenAnyGame
                ? _unseenAnyGameHtml
                : _unseenThisGameHtml;
            return "<color=" + colour + ">*</color>";
        }

        /// <summary>
        /// Walks an overflowed crawl a second time, keeping the tally that says where it
        /// blew up.
        /// </summary>
        /// <remarks>
        /// Only overflows are walked twice, and only when the overflow log is on. The
        /// alternative - keeping the tally on every crawl - would make the common case
        /// pay for a report it will never produce. The re-walk reaches the same place
        /// because <see cref="GameLookAheadWorld"/> caches every read it makes, so the
        /// second pass is answered from the snapshot the first one took.
        /// </remarks>
        private static LookAheadTrace? TraceOverflow(
            LookAheadGraph graph,
            DialogueNodeId start,
            GameLookAheadWorld world,
            GlobalStateSession session,
            LookAheadResult result)
        {
            LookAheadEngine? tracing = _tracingEngine;
            if (tracing == null || !result.BudgetExhausted)
            {
                return null;
            }

            return tracing.Evaluate(
                graph,
                start,
                world,
                node => NoveltyOf(session, node.ConversationId, node.EntryId)).Trace;
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
            _comparison?.Report();
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
            private static void Prefix(Response[] responses)
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
                    if (marker != null)
                    {
                        __result.responseText += marker;
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
