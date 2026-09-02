// SPDX-License-Identifier: MIT
using System;
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
            _failures = new HookFailureLimiter(
                "marking options that still lead somewhere unread", log);
            _unseenAnyGameHtml = Validate(unseenAnyGameHtml, nameof(unseenAnyGameHtml));
            _unseenThisGameHtml = Validate(unseenThisGameHtml, nameof(unseenThisGameHtml));
            Configure(enabled, stateBudget, timeBudgetMs, diagnostics);

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

            if (_diagnostics != null)
            {
                _diagnostics.Record(
                    start, result, Milliseconds(ticks), _budget,
                    TraceOverflow(graph, start, world, session, result));
            }

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
