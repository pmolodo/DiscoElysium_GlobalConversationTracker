// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Engine
{
    /// <summary>One thing the engine wants READ rather than evaluated.</summary>
    /// <remarks>
    /// <para>The other way the engine asks - <see cref="LookAheadQuestions.Queries"/> - hands
    /// over a rendered Lua call to RUN. That is exact, because the game answers it, and it is
    /// also how a guard calling <c>FinishTask</c> came to close a journal task every time a
    /// response menu opened. A kind names DATA instead, so servicing one is a read and cannot
    /// be anything else.</para>
    ///
    /// <para>A kind that asks about one thing names it in <see cref="Subject"/>. A kind that
    /// answers with a whole set leaves it empty - the thought cabinet is read that way,
    /// because the game holds it as two collections and walking them once is cheaper and
    /// simpler than a request per thought.</para>
    /// </remarks>
    public sealed class DataRequest
    {
        /// <summary>Creates a request.</summary>
        /// <param name="kind">What is being asked for.</param>
        /// <param name="subject">What it is about, or empty for a set-valued kind.</param>
        public DataRequest(DataKind kind, string subject)
        {
            Kind = kind;
            Subject = subject;
        }

        /// <summary>What is being asked for - the vocabulary both sides share.</summary>
        public DataKind Kind { get; }

        /// <summary>What it is about, or empty where the kind answers with a set.</summary>
        /// <remarks>
        /// A string, though its values are as closed a set as <see cref="DataKind"/> is:
        /// item and thought names are derived game data, versioned with the content rather
        /// than with this protocol, and they already cross as strings in
        /// <see cref="LookAheadQuestions.Items"/> and its neighbours.
        /// </remarks>
        public string Subject { get; }
    }
}
