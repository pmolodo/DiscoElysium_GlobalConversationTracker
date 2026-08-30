// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>The dialogue entries the look-ahead can walk, indexed by id.</summary>
    /// <remarks>
    /// Holds a whole conversation group rather than one conversation. Links cross
    /// conversation boundaries, and the closure matters: crawling from the 511 entries
    /// of WHIRLING / LENA INTRO reaches 1,844 nodes across three conversations, of which
    /// only 511 are in the starting one. Clipping at the conversation boundary would
    /// discard 72% of the region.
    /// </remarks>
    public sealed class LookAheadGraph
    {
        private readonly Dictionary<DialogueNodeId, LookAheadNode> _nodes;

        /// <summary>Creates a graph.</summary>
        /// <param name="nodes">The entries, which must have distinct ids.</param>
        /// <param name="symbols">The table their actions were interned into.</param>
        /// <exception cref="ArgumentNullException">An argument is null.</exception>
        /// <exception cref="ArgumentException">Two entries share an id.</exception>
        public LookAheadGraph(IEnumerable<LookAheadNode> nodes, StateSymbols symbols)
        {
            if (nodes == null)
            {
                throw new ArgumentNullException(nameof(nodes));
            }

            Symbols = symbols ?? throw new ArgumentNullException(nameof(symbols));
            _nodes = new Dictionary<DialogueNodeId, LookAheadNode>();
            foreach (LookAheadNode node in nodes)
            {
                if (_nodes.ContainsKey(node.Id))
                {
                    throw new ArgumentException(
                        $"Duplicate dialogue entry {node.Id.ConversationId}:{node.Id.EntryId}.",
                        nameof(nodes));
                }

                _nodes.Add(node.Id, node);
            }
        }

        /// <summary>The table this graph's slot indices belong to.</summary>
        public StateSymbols Symbols { get; }

        /// <summary>How many entries the graph holds.</summary>
        public int Count => _nodes.Count;

        /// <summary>Every entry.</summary>
        public IEnumerable<LookAheadNode> Nodes => _nodes.Values;

        /// <summary>Looks an entry up.</summary>
        /// <param name="id">Which entry.</param>
        /// <param name="node">The entry, when this returns true.</param>
        public bool TryGet(DialogueNodeId id, out LookAheadNode node)
        {
            return _nodes.TryGetValue(id, out node!);
        }

        /// <summary>Looks an entry up, or throws.</summary>
        /// <param name="id">Which entry.</param>
        /// <exception cref="KeyNotFoundException">No such entry.</exception>
        public LookAheadNode Get(DialogueNodeId id)
        {
            if (!_nodes.TryGetValue(id, out LookAheadNode? node))
            {
                throw new KeyNotFoundException(
                    $"No dialogue entry {id.ConversationId}:{id.EntryId} in this graph.");
            }

            return node;
        }

        /// <summary>Whether the graph holds an entry.</summary>
        /// <param name="id">Which entry.</param>
        public bool Contains(DialogueNodeId id)
        {
            return _nodes.ContainsKey(id);
        }
    }
}
