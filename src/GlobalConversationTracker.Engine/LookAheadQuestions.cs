// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// Everything a crawl over one conversation group can ask the world, as the engine
    /// named it.
    /// </summary>
    /// <remarks>
    /// <para>THE ENGINE ASKS THE QUESTIONS, and this is the list. Every answer must come
    /// back under the exact key given here - see <see cref="WorldSnapshot"/> - because the
    /// alternative was for both sides to render every call identically forever, including how a number is formatted and how a string is
    /// escaped. One disagreement there and the answer silently goes missing, the query
    /// reads Unknown, the guard turns permissive, and the marker is wrong with nothing to
    /// report it.</para>
    ///
    /// <para>Worth caching against a conversation. The questions cannot change while the
    /// game is running, the walk over a group's guards is not free, and the lists are long
    /// - 306 variables and 4,514 entries for conversation 631.</para>
    /// </remarks>
    public sealed class LookAheadQuestions
    {
        private LookAheadQuestions(
            IReadOnlyList<int> conversations,
            IReadOnlyList<string> variables,
            IReadOnlyList<string> queries,
            IReadOnlyList<string> items,
            IReadOnlyList<string> thoughts,
            IReadOnlyList<NodeRef> checks,
            IReadOnlyList<NodeRef> entries,
            IReadOnlyList<DataRequest> data)
        {
            Conversations = conversations;
            Variables = variables;
            Queries = queries;
            Items = items;
            Thoughts = thoughts;
            Checks = checks;
            Entries = entries;
            Data = data;
        }

        /// <summary>
        /// The conversations the group covers, so the caller knows what it committed to.
        /// </summary>
        public IReadOnlyList<int> Conversations { get; }

        /// <summary>Dialogue variables some guard in the group reads, by name.</summary>
        public IReadOnlyList<string> Variables { get; }

        /// <summary>
        /// World queries, by the key their answers must come back under - the rendered
        /// call, such as <c>IsKimHere()</c>.
        /// </summary>
        public IReadOnlyList<string> Queries { get; }

        /// <summary>Items some guard asks about, by name.</summary>
        public IReadOnlyList<string> Items { get; }

        /// <summary>Thoughts some guard asks about, by name.</summary>
        public IReadOnlyList<string> Thoughts { get; }

        /// <summary>Entries carrying a skill check, whose outcome the world decides.</summary>
        public IReadOnlyList<NodeRef> Checks { get; }

        /// <summary>Every entry in the group, because any of them may have been seen.</summary>
        public IReadOnlyList<NodeRef> Entries { get; }

        /// <summary>World state the engine wants READ rather than evaluated.</summary>
        /// <remarks>
        /// Answered in this order, as <see cref="WorldSnapshot.DataValues"/>. Unlike
        /// <see cref="Queries"/>, servicing one of these is a read of game state and never
        /// the running of a dialogue function - see <see cref="DataRequest"/>.
        /// </remarks>
        public IReadOnlyList<DataRequest> Data { get; }

        /// <summary>Composes the questions from what crossed.</summary>
        /// <remarks>
        /// For <c>WireConvert</c>, which is the only thing that builds one: these are what
        /// the engine asked, never something this side decides. The constructor stays
        /// private so that stays true.
        /// </remarks>
        /// <param name="conversations">The conversations the group covers.</param>
        /// <param name="variables">Dialogue variables read by some guard.</param>
        /// <param name="queries">World queries, by the key their answers come back under.</param>
        /// <param name="items">Items some guard asks about.</param>
        /// <param name="thoughts">Thoughts some guard asks about.</param>
        /// <param name="checks">Entries carrying a skill check.</param>
        /// <param name="entries">Every entry, because any may have been seen.</param>
        /// <param name="data">World state the engine wants read rather than evaluated.</param>
        internal static LookAheadQuestions Of(
            IReadOnlyList<int> conversations,
            IReadOnlyList<string> variables,
            IReadOnlyList<string> queries,
            IReadOnlyList<string> items,
            IReadOnlyList<string> thoughts,
            IReadOnlyList<NodeRef> checks,
            IReadOnlyList<NodeRef> entries,
            IReadOnlyList<DataRequest> data)
        {
            return new LookAheadQuestions(
                conversations, variables, queries, items, thoughts, checks, entries, data);
        }
    }
}
