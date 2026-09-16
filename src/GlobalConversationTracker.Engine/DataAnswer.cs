// SPDX-License-Identifier: MIT
using System.Collections.Generic;

namespace GlobalConversationTracker.Engine
{
    /// <summary>What was found for one <see cref="DataRequest"/>.</summary>
    /// <remarks>
    /// <para>Two shapes in one, because a kind answers in one or the other and which it uses
    /// is part of what the kind means: a value for a kind about one subject, a set of names
    /// for a kind that enumerates.</para>
    ///
    /// <para>ANSWER NOTHING YOU CANNOT ANSWER, and say which it is. Leaving
    /// <see cref="Read"/> false is how a request that could not be serviced is reported, and
    /// it matters because an empty <see cref="Names"/> would otherwise mean two opposite
    /// things - a set that really is empty, and a set nobody could reach. The engine reads
    /// the first as "the subject is not in it" and the second as "not knowable", and
    /// answering the first when it is the second CLOSES routes the game opens.</para>
    /// </remarks>
    public sealed class DataAnswer
    {
        /// <summary>An answer that could not be given.</summary>
        public static DataAnswer Unreadable()
        {
            return new DataAnswer();
        }

        /// <summary>An answer that is a whole set of names, read successfully.</summary>
        /// <param name="names">Everything in the set.</param>
        public static DataAnswer OfNames(IEnumerable<string> names)
        {
            var answer = new DataAnswer { Read = true };
            foreach (string name in names)
            {
                answer.Names.Add(name);
            }

            return answer;
        }

        /// <summary>An answer about one subject.</summary>
        /// <param name="value">What the subject is.</param>
        public static DataAnswer Of(WireValue value)
        {
            return new DataAnswer { Value = value, Read = true };
        }

        /// <summary>For a kind that answers about one subject.</summary>
        public WireValue Value { get; private set; } = WireValue.Unknown;

        /// <summary>For a kind that answers with a set of names.</summary>
        public IList<string> Names { get; } = new List<string>();

        /// <summary>Whether the request was serviced at all.</summary>
        public bool Read { get; private set; }
    }
}
