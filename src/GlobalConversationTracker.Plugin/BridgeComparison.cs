// SPDX-License-Identifier: MIT
using System.Globalization;
using GlobalConversationTracker.Engine;
using GlobalConversationTracker.LookAhead;
using GlobalConversationTracker.Session;

namespace GlobalConversationTracker
{
    /// <summary>
    /// What the two look-ahead engines said about the same option, over real play.
    /// </summary>
    /// <remarks>
    /// <para>The look-ahead is moving from C# to Rust, and the step before the C# can be
    /// deleted is evidence rather than confidence. Both engines run for a while, the
    /// managed one's answer is the one the player sees, and every disagreement is written
    /// down. When the log goes quiet the marker switches over; until then the migration
    /// costs a wrong answer to nobody.</para>
    ///
    /// <para>It is nearly free. The managed crawl was being run anyway and the bridge call
    /// is one per response menu, so what this adds is a dictionary lookup per option and a
    /// comparison.</para>
    ///
    /// <para>The counts matter as much as the disagreements. A comparison that reports
    /// nothing wrong because it never ran is the failure mode to avoid, so the summary
    /// always says HOW MANY were compared - and how long each side took, which is the other
    /// thing nobody has measured: a bridge that answers in two milliseconds and spends ten
    /// marshalling has not helped.</para>
    /// </remarks>
    internal sealed class BridgeComparison
    {
        /// <summary>The prefix every line here starts with, so a harness can find them.</summary>
        internal const string LogPrefix = "Look-ahead bridge:";

        /// <summary>
        /// How many disagreements are reported one by one before only the count is kept.
        /// </summary>
        /// <remarks>
        /// A disagreement is nearly always systematic, so the first few say what the
        /// hundredth would, and a line per option would bury the summary in a log the
        /// player may be reading for something else entirely.
        /// </remarks>
        private const int NamedDisagreements = 10;

        private readonly IGlobalStateLog _log;

        private int _compared;
        private int _disagreed;
        private int _unanswered;
        private int _incomplete;
        private int _notCrawled;
        private int _menus;
        private double _bridgeMilliseconds;
        private double _managedMilliseconds;

        /// <summary>Creates a comparison that reports to <paramref name="log"/>.</summary>
        /// <param name="log">Where disagreements and the summary go.</param>
        internal BridgeComparison(IGlobalStateLog log)
        {
            _log = log;
        }

        /// <summary>Whether anything has been compared at all.</summary>
        internal bool Ran => _compared > 0 || _menus > 0 || _notCrawled > 0;

        /// <summary>
        /// Records an option the managed engine answered WITHOUT crawling.
        /// </summary>
        /// <remarks>
        /// Counted rather than compared, and the difference matters. The managed engine
        /// stops early in two cases - the option is already the most novel thing there is,
        /// or no entry in its graph outranks it - and neither produces a best-reachable
        /// figure, only the knowledge that none can beat what the option already shows. The
        /// bridge crawls anyway and returns a real one, which is often legitimately LOWER.
        /// Comparing the two numbers there would report a disagreement on every such option
        /// and mean nothing by it.
        /// </remarks>
        internal void NotCrawled()
        {
            _notCrawled++;
        }

        /// <summary>Records what one bridge call cost, for a whole menu.</summary>
        /// <param name="milliseconds">The whole round trip: build, cross, crawl, parse.</param>
        internal void RecordMenu(double milliseconds)
        {
            _menus++;
            _bridgeMilliseconds += milliseconds;
        }

        /// <summary>Records the two answers for one option.</summary>
        /// <param name="start">The option.</param>
        /// <param name="managed">What the managed engine found.</param>
        /// <param name="managedMilliseconds">What the managed crawl cost.</param>
        /// <param name="bridge">
        /// What the bridge found, or null where it had nothing to say - the library is not
        /// there, the menu call failed, or this option was not in the group it asked about.
        /// </param>
        internal void Record(
            DialogueNodeId start,
            Novelty managed,
            double managedMilliseconds,
            LookAheadAnswer? bridge)
        {
            _managedMilliseconds += managedMilliseconds;

            if (bridge == null)
            {
                _unanswered++;
                return;
            }

            LookAheadAnswer answer = bridge.Value;
            _compared++;
            if (!answer.Complete)
            {
                // Not a disagreement. The bridge's answer is a lower bound when its search
                // ran out, so a smaller one is the honest report of a budget rather than a
                // difference of opinion - and it is what de-pvq is about showing.
                _incomplete++;
            }

            if (answer.Best == (int)managed)
            {
                return;
            }

            _disagreed++;
            if (_disagreed <= NamedDisagreements)
            {
                _log.Warning(
                    $"{LogPrefix} {start.ConversationId}:{start.EntryId} - managed says "
                    + $"{managed}, bridge says {Describe(answer.Best)}"
                    + (answer.Complete ? string.Empty : " (its search was cut short)")
                    + ". The managed answer is the one being drawn.");
            }
        }

        /// <summary>Writes the summary. Call when a run or a suite ends.</summary>
        /// <remarks>
        /// Always written once something ran, agreement or not. "Nothing to report" and
        /// "nothing was compared" look identical from outside otherwise, and only one of
        /// them is good news.
        /// </remarks>
        internal void Report()
        {
            if (!Ran)
            {
                return;
            }

            string timing = _menus == 0 || _compared == 0
                ? "no timings"
                : $"{Mean(_bridgeMilliseconds, _menus):N1} ms per menu across the bridge "
                    + $"against {Mean(_managedMilliseconds, _compared):N1} ms per option in "
                    + "the managed engine";

            string line =
                $"{LogPrefix} {_compared} options compared over {_menus} menus, "
                + $"{_disagreed} disagreed, {_incomplete} answered from a cut-short search, "
                + $"{_unanswered} the bridge could not answer, "
                + $"{_notCrawled} the managed engine answered without crawling. {timing}.";

            if (_disagreed > 0)
            {
                _log.Warning(line);
            }
            else
            {
                _log.Info(line);
            }
        }

        private static double Mean(double total, int count)
        {
            return count == 0 ? 0 : total / count;
        }

        /// <summary>A novelty code as the managed enum spells it.</summary>
        private static string Describe(int best)
        {
            return best >= 0 && best <= (int)Novelty.UnseenAnyGame
                ? ((Novelty)best).ToString()
                : best.ToString(CultureInfo.InvariantCulture);
        }
    }
}
