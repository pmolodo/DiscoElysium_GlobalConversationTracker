// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.Engine
{
    /// <summary>What one option scored.</summary>
    /// <param name="Start">The option entry that was asked about.</param>
    /// <param name="Best">
    /// The best novelty reachable from it: 0 seen, 1 unseen this save, 2 unseen in any
    /// save. The same numbering as the managed engine's <c>Novelty</c>, deliberately, so
    /// the two can be compared while both exist.
    /// </param>
    /// <param name="Complete">
    /// Whether the search settled. False means <paramref name="Best"/> is a LOWER BOUND -
    /// the crawl ran out of budget, and something better may be reachable. A marker drawn
    /// from an incomplete answer is not wrong, only possibly too modest.
    /// </param>
    /// <param name="ElapsedMs">How long the crawl for this option took.</param>
    /// <param name="StatesExplored">
    /// How many search states it explored. What the diagnostics are really about: a time
    /// alone cannot say whether a menu was slow because the search was large or because the
    /// machine was busy, and this is the half that is the same on both.
    /// </param>
    /// <param name="NodesReached">How many entries it reached.</param>
    /// <param name="StoppedBy">
    /// What stopped it: "none", "states" or "time". More than
    /// <paramref name="Complete"/> says, and the difference is the one a player tuning the
    /// budgets needs - a crawl out of STATES wants a bigger state budget, one out of TIME
    /// on the same states wants a longer clock.
    /// </param>
    /// <param name="Branch">
    /// Which outcome of a rolled check this answers for, <see cref="LookAheadAnswer.Pass"/>
    /// or <see cref="LookAheadAnswer.Fail"/>, and null on an ordinary option. The absence
    /// is what the mod reads to decide whether an option earns a Pass / Fail line at all -
    /// see de-fes - and an older library that has never named an outcome reads as exactly
    /// that.
    /// </param>
    /// <param name="Destination">
    /// The best novelty already known about this start before any search: the option's own
    /// novelty, or for an outcome the best among the entries it leads to DIRECTLY. What the
    /// mod colours the word by, and the baseline a crawl has to beat to be worth running.
    /// </param>
    public readonly record struct LookAheadAnswer(
        NodeRef Start,
        int Best,
        bool Complete,
        long ElapsedMs,
        long StatesExplored,
        long NodesReached,
        string StoppedBy,
        string? Branch = null,
        int Destination = 0)
    {
        /// <summary>What the outcome where a check succeeds is called on the wire.</summary>
        public const string Pass = "pass";

        /// <summary>And the one where it fails.</summary>
        public const string Fail = "fail";
    }

    /// <summary>Both answers a rolled check came back as.</summary>
    /// <param name="Pass">The outcome where the check succeeds.</param>
    /// <param name="Fail">The one where it fails.</param>
    public readonly record struct Outcomes(LookAheadAnswer Pass, LookAheadAnswer Fail);

    /// <summary>What the library said about a whole menu.</summary>
    /// <remarks>
    /// A request the engine could not serve at all comes back with <see cref="Error"/> set
    /// and no answers, rather than as a thrown exception, so a caller has ONE thing to
    /// parse and one place to look. What throws is what happens before there is a response
    /// at all: a request that is not JSON, or a library that is not there.
    /// </remarks>
    public sealed class LookAheadResponse
    {
        private LookAheadResponse(IReadOnlyList<LookAheadAnswer> answers, string? error)
        {
            Answers = answers;
            Error = error;
        }

        /// <summary>
        /// One per thing that can be chosen, in the order the starts were asked about.
        /// </summary>
        /// <remarks>
        /// NOT ONE PER START. A rolled check is two options wearing one line of text, so it
        /// comes back as TWO of these - see <see cref="LookAheadAnswer.Branch"/> - and a
        /// menu of n options with k checks is answered by n + k. Indexing this against the
        /// starts that were sent will not line up; look an answer up by what it is for,
        /// with <see cref="Find"/> or <see cref="OutcomesOf"/>.
        /// </remarks>
        public IReadOnlyList<LookAheadAnswer> Answers { get; }

        /// <summary>Why the whole request failed, or null if it did not.</summary>
        public string? Error { get; }

        /// <summary>The answer for one start, or for one outcome of it.</summary>
        /// <remarks>
        /// Getting it wrong is quiet rather than wrong: asking for a rolled check with no
        /// outcome named matches nothing, which is the honest answer rather than the pass
        /// half by accident.
        /// </remarks>
        /// <param name="start">The entry.</param>
        /// <param name="branch">
        /// <see cref="LookAheadAnswer.Pass"/>, <see cref="LookAheadAnswer.Fail"/>, or null
        /// for an ordinary option.
        /// </param>
        /// <returns>The answer, or null if there is none.</returns>
        public LookAheadAnswer? Find(NodeRef start, string? branch)
        {
            foreach (LookAheadAnswer answer in Answers)
            {
                if (answer.Start.Equals(start)
                    && string.Equals(answer.Branch, branch, StringComparison.Ordinal))
                {
                    return answer;
                }
            }

            return null;
        }

        /// <summary>Both outcomes of a rolled check, or null where the start is not one.</summary>
        /// <remarks>
        /// The null is what the mod reads to decide whether an option earns a Pass / Fail
        /// line at all, which is the job the absent nested pair used to do.
        /// </remarks>
        /// <param name="start">The entry.</param>
        /// <returns>The pair, or null.</returns>
        public Outcomes? OutcomesOf(NodeRef start)
        {
            LookAheadAnswer? pass = Find(start, LookAheadAnswer.Pass);
            LookAheadAnswer? fail = Find(start, LookAheadAnswer.Fail);

            return pass is LookAheadAnswer passed && fail is LookAheadAnswer failed
                ? new Outcomes(passed, failed)
                : null;
        }

        /// <summary>Composes a response from what crossed.</summary>
        /// <remarks>
        /// For <c>WireConvert</c>, which is the only thing that builds one: a response is
        /// something the engine said, never something this side makes up. The constructor
        /// stays private so that stays true.
        /// </remarks>
        /// <param name="answers">One per thing that can be chosen.</param>
        /// <param name="error">Why the whole request failed, or null if it did not.</param>
        /// <exception cref="ArgumentNullException">The answers are null.</exception>
        internal static LookAheadResponse Of(
            IReadOnlyList<LookAheadAnswer> answers, string? error)
        {
            if (answers == null)
            {
                throw new ArgumentNullException(nameof(answers));
            }

            return new LookAheadResponse(answers, error);
        }
    }
}
