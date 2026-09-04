// SPDX-License-Identifier: MIT
using System.Collections;
using System.Collections.Generic;
using System.Globalization;

using GlobalConversationTracker.Core;
using System.Text;
using System.Text.Json;

namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// A set of dialogue entries, in the shape it crosses the bridge in.
    /// </summary>
    /// <remarks>
    /// <para>A request names entries three times over - what the player has been shown,
    /// what is unseen this game, what is unseen in any game - and a conversation group is
    /// not small: 4,514 entries for conversation 631. So the shape is chosen rather than
    /// assumed. Measured, one such set costs:</para>
    /// <code>
    /// one entry set                            members   objects      ids     runs    bits
    /// everything seen (a completionist save)      4514    149040    18027      146     754
    /// one entry in ten, clustered                  451     14774     1703       16     754
    /// one entry in ten, scattered                  452     14925     1850     1850     754
    /// </code>
    /// <para>where <c>objects</c> is a list of <c>{"conversation":631,"entry":12}</c>,
    /// <c>ids</c> groups entry ids by conversation, <c>runs</c> is this, and <c>bits</c>
    /// is a base64 bitmap. A whole request went from 341,357 bytes to 22,583 by choosing
    /// runs, and it is built inside the frame that draws a response menu.</para>
    ///
    /// <para>Runs win because a save's history is CLUSTERED - a player walks through a
    /// conversation rather than through every seventh entry of one. The bitmap is smaller
    /// in the scattered case and was still not chosen: it is the only shape whose meaning
    /// lives outside itself, so a stale questions list would decode cleanly and mean
    /// something else.</para>
    ///
    /// <para>The written form, which the Rust side reads back:</para>
    /// <code>
    /// {"631":"0-40,42,50-99","636":"3"}
    /// </code>
    /// </remarks>
    public sealed class NodeSet : IEnumerable<NodeRef>
    {
        private readonly HashSet<NodeRef> _nodes = new HashSet<NodeRef>();

        /// <summary>How many entries are in the set.</summary>
        public int Count => _nodes.Count;

        /// <summary>Adds an entry. Adding one twice is not an error.</summary>
        /// <param name="node">The entry.</param>
        public void Add(NodeRef node)
        {
            _nodes.Add(node);
        }

        /// <summary>Whether the set holds an entry.</summary>
        /// <param name="node">The entry.</param>
        public bool Contains(NodeRef node)
        {
            return _nodes.Contains(node);
        }

        /// <inheritdoc/>
        public IEnumerator<NodeRef> GetEnumerator() => _nodes.GetEnumerator();

        /// <inheritdoc/>
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();

        /// <summary>Writes the set as the value of the property already begun.</summary>
        /// <param name="writer">The writer, positioned to take a value.</param>
        public void Write(Utf8JsonWriter writer)
        {
            writer.WriteStartObject();
            foreach (KeyValuePair<int, List<int>> conversation in ByConversation())
            {
                writer.WriteString(
                    conversation.Key.ToString(CultureInfo.InvariantCulture),
                    Runs(conversation.Value));
            }

            writer.WriteEndObject();
        }

        /// <summary>The set as its own JSON object. For tests and for diagnostics.</summary>
        public string ToJson()
        {
            var buffer = new System.IO.MemoryStream();
            using (var writer = new Utf8JsonWriter(buffer))
            {
                Write(writer);
            }

            return Encoding.UTF8.GetString(buffer.ToArray());
        }

        /// <summary>The entry ids of each conversation, ascending.</summary>
        /// <remarks>
        /// Sorted by conversation as well as within one, so the same set always writes the
        /// same bytes. Two requests that differ only in dictionary iteration order would
        /// otherwise be impossible to compare, and comparing them is how the in-game check
        /// works.
        /// </remarks>
        private SortedDictionary<int, List<int>> ByConversation()
        {
            var grouped = new SortedDictionary<int, List<int>>();
            foreach (NodeRef node in _nodes)
            {
                if (!grouped.TryGetValue(node.Conversation, out List<int>? entries))
                {
                    entries = new List<int>();
                    grouped[node.Conversation] = entries;
                }

                entries.Add(node.Entry);
            }

            foreach (List<int> entries in grouped.Values)
            {
                entries.Sort();
            }

            return grouped;
        }

        /// <summary>Ascending ids as <c>0-3,5,9-10</c>.</summary>
        /// <remarks>
        /// THROUGH THE ROUTINE EVERY FILE HERE USES, rather than the copy this class kept.
        /// The encoding is the same idea in both places, and it had three implementations -
        /// one on each side of the bridge and one for the files - which is three chances
        /// for a run to come to mean something slightly different. The spelling moved with
        /// it, from <c>..</c> to the hyphen; see <c>RUN_SEPARATOR</c> in the Rust bridge
        /// for why the reason behind the old one did not hold.
        /// </remarks>
        /// <param name="entries">The ids, ascending and without duplicates.</param>
        private static string Runs(List<int> entries) =>
            SparseOrder.PackRange(entries.ConvertAll(id => (long)id));
    }
}
