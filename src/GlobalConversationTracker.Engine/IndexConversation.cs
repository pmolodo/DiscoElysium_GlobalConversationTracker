// SPDX-License-Identifier: MIT
using System.Collections.Generic;

namespace GlobalConversationTracker.Engine
{
    /// <summary>One dialogue entry, as an index carries it.</summary>
    /// <remarks>
    /// Plain data on purpose. It is filled from the live dialogue database by the plugin
    /// and written out by <see cref="ShippedIndexWriter"/>, and neither of those should
    /// need to know about the other's world - which is also what lets the writer be tested
    /// without a game in front of it.
    /// </remarks>
    public sealed class IndexEntry
    {
        /// <summary>The entry id, unique within its conversation.</summary>
        public int Id { get; set; }

        /// <summary>Whether it is a group node rather than a selectable line.</summary>
        public bool Group { get; set; }

        /// <summary>Its conditions, verbatim.</summary>
        public string Guard { get; set; } = string.Empty;

        /// <summary>Its user script, verbatim.</summary>
        public string Script { get; set; } = string.Empty;

        /// <summary>
        /// Where its outgoing links go, as (conversation, entry) pairs, in order.
        /// </summary>
        /// <remarks>
        /// Both halves always, even for a link that stays inside its own conversation. The
        /// index writes the conversation half as an absent key in that case, and
        /// <see cref="ShippedIndexWriter"/> decides that when it writes; nothing upstream
        /// of it has to.
        /// </remarks>
        public IList<KeyValuePair<int, int>> Links { get; } =
            new List<KeyValuePair<int, int>>();

        /// <summary>Its fields, by name.</summary>
        /// <remarks>
        /// Only the ones in <see cref="IndexFields.Read"/> survive into an index; a caller
        /// may put everything it has here and let the writer drop the rest.
        /// </remarks>
        public IList<KeyValuePair<string, string>> Fields { get; } =
            new List<KeyValuePair<string, string>>();
    }

    /// <summary>One conversation, as an index carries it.</summary>
    public sealed class IndexConversation
    {
        /// <summary>Creates a conversation.</summary>
        /// <param name="id">The conversation id the game knows it by.</param>
        public IndexConversation(int id)
        {
            Id = id;
        }

        /// <summary>The conversation id.</summary>
        public int Id { get; }

        /// <summary>Its dialogue entries.</summary>
        public IList<IndexEntry> Entries { get; } = new List<IndexEntry>();

        /// <summary>Its own fields, by name.</summary>
        /// <remarks>
        /// Only the ones in <see cref="IndexFields.ConversationRead"/> survive into an index; a
        /// caller may put everything it has here and let the writer drop the rest.
        /// </remarks>
        public IList<KeyValuePair<string, string>> Fields { get; } =
            new List<KeyValuePair<string, string>>();

        /// <summary>What its content reduces to, for validating a cache of it.</summary>
        public string Hash()
        {
            var hasher = new ConversationHasher(Id);
            hasher.AddConversationFields(Fields);
            foreach (IndexEntry entry in Entries)
            {
                hasher.Add(entry.Id, entry.Group, entry.Guard, entry.Script, entry.Links, entry.Fields);
            }

            return hasher.Finish();
        }
    }
}
