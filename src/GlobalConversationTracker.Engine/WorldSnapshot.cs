// SPDX-License-Identifier: MIT
using System.Collections.Generic;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// The player's situation, as answers to the questions the engine asked.
    /// </summary>
    /// <remarks>
    /// <para>The world crosses as data rather than as callbacks. That is not a
    /// simplification of what the managed engine does - its world is built fresh per
    /// response menu and caches each distinct query for the life of the crawl, because a
    /// menu asks the same handful of questions hundreds of times as the search fans out.
    /// So the world a crawl sees is already a finite table of answers, and the set of
    /// questions is <see cref="LookAheadQuestions"/>.</para>
    ///
    /// <para>ANSWER NOTHING YOU CANNOT ANSWER. Anything left out reads Unknown on the
    /// other side, which is permissive: it widens the reachable set rather than narrowing
    /// it, costing a wasted click instead of hiding content the player has never seen. A
    /// guess does neither reliably.</para>
    /// </remarks>
    public sealed class WorldSnapshot
    {
        /// <summary>The player's balance in centimes, before the crawl.</summary>
        public int Money { get; set; }

        /// <summary>The clock as the crawl begins, in minutes since midnight.</summary>
        public int DayMinutes { get; set; }

        /// <summary>The story's day number, which a conversation cannot move.</summary>
        public int DayCounter { get; set; }

        /// <summary>
        /// Whether the clock is locked, which makes <c>PassTime()</c> a no-op.
        /// </summary>
        public bool ClockLocked { get; set; }

        /// <summary>
        /// The dialogue variables, answered in the order
        /// <see cref="LookAheadQuestions.Variables"/> listed their names.
        /// </summary>
        /// <remarks>
        /// <para>BY POSITION, not by name, and the names are exactly why. They are constant
        /// for a group and the caller already holds them - it asked for the questions once
        /// and cached them, because they cannot change while the game is running - so
        /// sending them back with every response menu is sending back what the engine
        /// itself said. Measured, the 306 names of conversation 631's group were most of a
        /// 22,622-byte request; answering by position makes it 12,315.</para>
        ///
        /// <para>The list must be either EMPTY or exactly as long as the questions asked.
        /// A list of any other length is refused outright by the engine rather than zipped
        /// as far as it goes, because a caller answering a stale questions list would
        /// otherwise have every answer after the first difference land on the wrong
        /// variable.</para>
        ///
        /// <para>Answer <see cref="WireValue.Unknown"/> for a variable the game would not
        /// give up. That is not a hole: where the variable table has been deployed it falls
        /// back to what the DATABASE declares the variable to be, which is a better answer
        /// than "no idea" and the only one that gets a counter's kind right.</para>
        /// </remarks>
        public IList<WireValue> VariableValues { get; } = new List<WireValue>();

        /// <summary>
        /// The world queries, answered in the order
        /// <see cref="LookAheadQuestions.Queries"/> listed their keys.
        /// </summary>
        public IList<WireValue> QueryValues { get; } = new List<WireValue>();

        /// <summary>
        /// What was read for each of <see cref="LookAheadQuestions.Data"/>, in that order.
        /// </summary>
        /// <remarks>
        /// By position and subject to the same length rule as
        /// <see cref="VariableValues"/>: either empty, or exactly as long as the requests
        /// asked. A request that could not be serviced still takes its place in the list, as
        /// <see cref="DataAnswer.Unreadable"/> - leaving it out would shift every answer
        /// after it onto the wrong request.
        /// </remarks>
        public IList<DataAnswer> DataValues { get; } = new List<DataAnswer>();

        /// <summary>Items held when the crawl starts.</summary>
        public ISet<string> Items { get; } = new HashSet<string>();

        /// <summary>Journal tasks active when the crawl starts.</summary>
        public ISet<string> Tasks { get; } = new HashSet<string>();

        /// <summary>Thoughts in the cabinet when the crawl starts.</summary>
        public ISet<string> Thoughts { get; } = new HashSet<string>();

        /// <summary>Entries whose skill check the plugin says passes.</summary>
        /// <remarks>
        /// Two sets rather than one list of outcomes, because the third outcome is "not
        /// known", and an entry in neither set already says that.
        /// </remarks>
        public NodeSet ChecksPass { get; } = new NodeSet();

        /// <summary>Entries whose skill check the plugin says fails.</summary>
        public NodeSet ChecksFail { get; } = new NodeSet();

        /// <summary>Entries the player has already been shown in this save.</summary>
        public NodeSet Seen { get; } = new NodeSet();

        /// <summary>The flags of every white check the game holds as failed.</summary>
        /// <remarks>
        /// The game keeps these in a table of its own rather than in Lua, and refuses a failed
        /// white check for as long as it stays there. The engine reads each as that check's
        /// failure slot set, which closes the check both as an option and as a way through.
        /// </remarks>
        public ISet<string> FailedWhiteChecks { get; } = new HashSet<string>();

        /// <summary>Whether a thought forces every red check to fail.</summary>
        /// <remarks>
        /// The game's <c>ThoughtAlterant.RedChecksFail</c>. While it is set a red check's
        /// success is closed to every crawl, and the check's Pass half is answered only for
        /// what passing would open.
        /// </remarks>
        public bool RedChecksFail { get; set; }

    }
}
