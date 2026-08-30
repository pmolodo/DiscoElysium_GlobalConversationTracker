// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using GlobalConversationTracker.LookAhead;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>Builds small graphs, written the way the database writes them.</summary>
    /// <remarks>
    /// Guards and actions are given as the raw <c>conditionsString</c> and
    /// <c>userScript</c> text rather than as pre-built objects, so the tests exercise
    /// the parsers on the same syntax the game ships and a fixture can be pasted
    /// straight out of the asset.
    /// </remarks>
    internal sealed class GraphBuilder
    {
        private const int DefaultConversation = 1;

        private readonly List<LookAheadNode> _nodes = new List<LookAheadNode>();

        public StateSymbols Symbols { get; } = new StateSymbols();

        public static DialogueNodeId Node(int id, int conversation = DefaultConversation)
        {
            return new DialogueNodeId(conversation, id);
        }

        public GraphBuilder Add(
            int id,
            string? guard = null,
            string? script = null,
            int[]? links = null,
            bool isGroup = false,
            bool isCheck = false,
            int cost = 0,
            bool costOnce = false,
            int conversation = DefaultConversation)
        {
            var targets = new List<DialogueNodeId>();
            if (links != null)
            {
                foreach (int link in links)
                {
                    targets.Add(Node(link, conversation));
                }
            }

            _nodes.Add(new LookAheadNode(
                Node(id, conversation),
                isGroup,
                isCheck,
                GuardParser.Parse(guard),
                ActionParser.Parse(script, Symbols),
                targets,
                cost,
                costOnce));
            return this;
        }

        /// <summary>Adds a node linking into another conversation.</summary>
        public GraphBuilder AddCrossing(
            int id, int conversation, DialogueNodeId[] links)
        {
            _nodes.Add(new LookAheadNode(
                Node(id, conversation),
                false,
                false,
                GuardExpression.AlwaysTrue,
                ActionParser.Parse(null, Symbols),
                links));
            return this;
        }

        public LookAheadGraph Build()
        {
            return new LookAheadGraph(_nodes, Symbols);
        }
    }
}
