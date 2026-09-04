// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Text.Json;

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

        /// <summary>Writes the snapshot as the value of the property already begun.</summary>
        /// <param name="writer">The writer, positioned to take a value.</param>
        public void Write(Utf8JsonWriter writer)
        {
            writer.WriteStartObject();

            writer.WriteNumber("money", Money);
            writer.WriteNumber("day_minutes", DayMinutes);
            writer.WriteNumber("day_counter", DayCounter);
            writer.WriteBoolean("clock_locked", ClockLocked);

            WriteValues(writer, "variable_values", VariableValues);
            WriteValues(writer, "query_values", QueryValues);
            WriteNames(writer, "items", Items);
            WriteNames(writer, "tasks", Tasks);
            WriteNames(writer, "thoughts", Thoughts);

            writer.WritePropertyName("checks_pass");
            ChecksPass.Write(writer);
            writer.WritePropertyName("checks_fail");
            ChecksFail.Write(writer);
            writer.WritePropertyName("seen");
            Seen.Write(writer);

            writer.WriteEndObject();
        }

        private static void WriteValues(
            Utf8JsonWriter writer, string name, IList<WireValue> values)
        {
            writer.WritePropertyName(name);
            writer.WriteStartArray();
            foreach (WireValue value in values)
            {
                value.Write(writer);
            }

            writer.WriteEndArray();
        }

        private static void WriteNames(Utf8JsonWriter writer, string name, ISet<string> names)
        {
            writer.WritePropertyName(name);
            writer.WriteStartArray();
            foreach (string member in names)
            {
                writer.WriteStringValue(member);
            }

            writer.WriteEndArray();
        }
    }
}
