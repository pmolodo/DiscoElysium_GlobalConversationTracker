// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using GlobalConversationTracker.LookAhead;
using PixelCrushers.DialogueSystem;

namespace GlobalConversationTracker
{
    /// <summary>
    /// Turns the game's dialogue database into the plain-data graph the look-ahead
    /// walks, one conversation group at a time.
    /// </summary>
    /// <remarks>
    /// <para>A group, not a conversation. Links cross conversation boundaries, and the
    /// closure matters: crawling from WHIRLING / LENA INTRO reaches 1,844 nodes across
    /// three conversations, only 511 of them in the starting one. Stopping at the
    /// boundary would discard nearly three quarters of the region and under-report on
    /// everything past it.</para>
    ///
    /// <para>Groups are small and closed - the two measured are 1,863 and 2,186 entries
    /// across three and two conversations - so building one is cheap, and the result is
    /// cached because the graph never changes while the game runs. Only the player's
    /// state does, and that is supplied separately per crawl.</para>
    /// </remarks>
    internal static class LookAheadGraphBuilder
    {
        /// <summary>The field marking each of the game's special node types.</summary>
        private const string PassiveField = "DifficultyPass";
        private const string RedField = "DifficultyRed";
        private const string WhiteField = "DifficultyWhite";
        private const string AtmoField = "DifficultyAtmo";
        private const string TestField = "HiddenTest";
        private const string KimWatchField = "kim_watch";
        private const string BooleanOnlyField = "boolean_only";
        private const string FlagNameField = "FlagName";
        private const string ClickCostField = "ClickCost";
        private const string HiddenNotEnoughField = "HiddenNotEnough";
        private const string CostOnceField = "CostOnce";

        /// <summary>The suffix a red check's failure flag carries.</summary>
        private const string FailedSuffix = "_failed";

        private static readonly Dictionary<int, LookAheadGraph> Cache =
            new Dictionary<int, LookAheadGraph>();

        /// <summary>Drops every cached graph. For a database swap or a config change.</summary>
        internal static void Clear()
        {
            Cache.Clear();
        }

        /// <summary>
        /// The graph for the group containing a conversation, building it on first use.
        /// </summary>
        /// <param name="database">The master dialogue database.</param>
        /// <param name="conversationId">Any conversation in the wanted group.</param>
        /// <returns>The graph, or null if the conversation is not in the database.</returns>
        internal static LookAheadGraph? ForConversation(
            DialogueDatabase database, int conversationId)
        {
            if (database == null)
            {
                return null;
            }

            if (Cache.TryGetValue(conversationId, out LookAheadGraph? cached))
            {
                return cached;
            }

            HashSet<int> group = DiscoverGroup(database, conversationId);
            if (group.Count == 0)
            {
                return null;
            }

            var symbols = new StateSymbols();
            var nodes = new List<LookAheadNode>();
            foreach (int id in group)
            {
                Conversation conversation = database.GetConversation(id);
                if (conversation == null)
                {
                    continue;
                }

                for (int i = 0; i < conversation.dialogueEntries.Count; i++)
                {
                    nodes.Add(Convert(conversation.dialogueEntries[i], symbols));
                }
            }

            var graph = new LookAheadGraph(nodes, symbols);

            // Every conversation in the group resolves to the same graph, so a later
            // menu in a neighbouring conversation reuses this build.
            foreach (int id in group)
            {
                Cache[id] = graph;
            }

            return graph;
        }

        /// <summary>
        /// The transitive closure of conversations reachable by following links out of
        /// this one.
        /// </summary>
        private static HashSet<int> DiscoverGroup(DialogueDatabase database, int start)
        {
            var group = new HashSet<int>();
            var pending = new Queue<int>();
            if (database.GetConversation(start) == null)
            {
                return group;
            }

            group.Add(start);
            pending.Enqueue(start);

            while (pending.Count > 0)
            {
                Conversation conversation = database.GetConversation(pending.Dequeue());
                if (conversation == null)
                {
                    continue;
                }

                for (int i = 0; i < conversation.dialogueEntries.Count; i++)
                {
                    DialogueEntry entry = conversation.dialogueEntries[i];
                    for (int j = 0; j < entry.outgoingLinks.Count; j++)
                    {
                        int destination = entry.outgoingLinks[j].destinationConversationID;
                        if (group.Add(destination))
                        {
                            pending.Enqueue(destination);
                        }
                    }
                }
            }

            return group;
        }

        private static LookAheadNode Convert(DialogueEntry entry, StateSymbols symbols)
        {
            var links = new List<DialogueNodeId>(entry.outgoingLinks.Count);
            for (int i = 0; i < entry.outgoingLinks.Count; i++)
            {
                Link link = entry.outgoingLinks[i];
                links.Add(new DialogueNodeId(
                    link.destinationConversationID, link.destinationDialogueID));
            }

            // A guard we cannot parse must not block: falling back to always-true
            // over-reports, and over-reporting is the survivable direction.
            GuardParser.TryParse(entry.conditionsString, out GuardExpression guard);

            DialogueCheckKind kind = KindOf(entry);
            int flagSlot = -1;
            int failedSlot = -1;
            if (kind == DialogueCheckKind.Red || kind == DialogueCheckKind.White)
            {
                string flag = Field.LookupValue(entry.fields, FlagNameField);
                if (!string.IsNullOrEmpty(flag))
                {
                    flagSlot = symbols.Variable(flag);
                    failedSlot = symbols.Variable(flag + FailedSuffix);
                }
            }

            int cost = Field.FieldExists(entry.fields, ClickCostField)
                ? Field.LookupInt(entry.fields, ClickCostField)
                : 0;

            return new LookAheadNode(
                new DialogueNodeId(entry.conversationID, entry.id),
                entry.isGroup,
                kind,
                guard,
                ActionParser.Parse(entry.userScript, symbols),
                links,
                cost < 0 ? 0 : cost,
                cost > 0 && Field.LookupBool(entry.fields, CostOnceField),
                cost > 0 && Field.LookupBool(entry.fields, HiddenNotEnoughField),
                flagSlot,
                failedSlot,
                kind == DialogueCheckKind.KimSwitch
                    && Field.LookupBool(entry.fields, BooleanOnlyField));
        }

        /// <summary>
        /// Which special node type an entry is, tested in the order
        /// <c>ReturnDialogueOptionValidator.IsEntryValid</c> tests them - it dispatches
        /// on the first match, so the order is behaviour, not style.
        /// </summary>
        private static DialogueCheckKind KindOf(DialogueEntry entry)
        {
            if (Field.FieldExists(entry.fields, PassiveField))
            {
                return DialogueCheckKind.Passive;
            }

            if (Field.FieldExists(entry.fields, RedField))
            {
                return DialogueCheckKind.Red;
            }

            if (Field.FieldExists(entry.fields, WhiteField))
            {
                return DialogueCheckKind.White;
            }

            if (Field.FieldExists(entry.fields, AtmoField))
            {
                return DialogueCheckKind.Fake;
            }

            if (Field.FieldExists(entry.fields, TestField))
            {
                return DialogueCheckKind.Test;
            }

            if (Field.FieldExists(entry.fields, KimWatchField))
            {
                return DialogueCheckKind.KimSwitch;
            }

            return DialogueCheckKind.None;
        }
    }
}
