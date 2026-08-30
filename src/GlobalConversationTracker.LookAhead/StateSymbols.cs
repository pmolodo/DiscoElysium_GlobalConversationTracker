// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>
    /// Maps every named thing the crawl can read or write onto a slot index, so a
    /// state is a small array of ints rather than a dictionary.
    /// </summary>
    /// <remarks>
    /// <para>One table for four kinds of name, distinguished by prefix: dialogue
    /// variables, inventory items, tasks, and per-node "already happened" markers. They
    /// share a table because they behave identically once named - a guard reads them, an
    /// action writes them - and because the search's inner loop wants one flat array to
    /// copy and hash, not four.</para>
    ///
    /// <para>Modelling only dialogue variables would be a bug, not a simplification.
    /// Conversation 451 gates the speakers on <c>CheckItem("samaran_speakers")</c> and
    /// unlocks them via <c>GainItem("shoes_faln")</c>, so inventory is read and written
    /// exactly the way variables are. Across the database that is 408 item mutations
    /// against 1,349 item guards, and 1,303 task mutations against 341 task guards.</para>
    /// </remarks>
    public sealed class StateSymbols
    {
        private readonly Dictionary<string, int> _indices =
            new Dictionary<string, int>(StringComparer.Ordinal);
        private readonly List<string> _names = new List<string>();

        /// <summary>Prefix for an inventory item's slot.</summary>
        public const string ItemPrefix = "item:";

        /// <summary>Prefix for a task's slot.</summary>
        public const string TaskPrefix = "task:";

        /// <summary>Prefix for a "this node already fired" marker slot.</summary>
        public const string OncePrefix = "once:";

        /// <summary>
        /// Prefix for a "this node has been displayed" slot, seeded from the save and
        /// then advanced speculatively as a path walks through it.
        /// </summary>
        public const string SeenPrefix = "seen:";

        /// <summary>How many slots exist.</summary>
        public int Count => _names.Count;

        /// <summary>The slot for a dialogue variable, creating it if new.</summary>
        /// <param name="name">The variable's name.</param>
        public int Variable(string name)
        {
            return Intern(name);
        }

        /// <summary>The slot for an inventory item, creating it if new.</summary>
        /// <param name="name">The item's name.</param>
        public int Item(string name)
        {
            return Intern(ItemPrefix + name);
        }

        /// <summary>The slot for a task, creating it if new.</summary>
        /// <param name="name">The task's name.</param>
        public int Task(string name)
        {
            return Intern(TaskPrefix + name);
        }

        /// <summary>The slot for a node's "already fired" marker, creating it if new.</summary>
        /// <param name="node">The node the marker belongs to.</param>
        public int Once(DialogueNodeId node)
        {
            return Intern(OncePrefix + node.ConversationId + ":" + node.EntryId);
        }

        /// <summary>The slot for a node's displayed marker, creating it if new.</summary>
        /// <param name="node">The node the marker belongs to.</param>
        public int Seen(DialogueNodeId node)
        {
            return Intern(SeenPrefix + node.ConversationId + ":" + node.EntryId);
        }

        /// <summary>The slot for a name already interned, or -1.</summary>
        /// <param name="name">The full slot name, prefix included.</param>
        public int Find(string name)
        {
            return _indices.TryGetValue(name, out int index) ? index : -1;
        }

        /// <summary>The name of a slot.</summary>
        /// <param name="index">The slot index.</param>
        public string NameOf(int index)
        {
            return _names[index];
        }

        private int Intern(string name)
        {
            if (_indices.TryGetValue(name, out int index))
            {
                return index;
            }

            index = _names.Count;
            _indices.Add(name, index);
            _names.Add(name);
            return index;
        }
    }
}
