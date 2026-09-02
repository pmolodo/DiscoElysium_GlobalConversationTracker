// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;
using GlobalConversationTracker.LookAhead;

namespace GlobalConversationTracker.LookAheadOffline
{
    internal sealed class ConversationIndex
    {
        private const string PassiveField = "DifficultyPass";
        private const string RedField = "DifficultyRed";
        private const string WhiteField = "DifficultyWhite";
        private const string FakeField = "DifficultyAtmo";
        private const string TestField = "HiddenTest";
        private const string KimWatchField = "kim_watch";
        private const string BooleanOnlyField = "boolean_only";
        private const string FlagNameField = "FlagName";
        private const string ClickCostField = "ClickCost";
        private const string CostOnceField = "CostOnce";
        private const string HiddenNotEnoughField = "HiddenNotEnough";

        private readonly Dictionary<int, ConversationRecord> _conversations;

        private ConversationIndex(Dictionary<int, ConversationRecord> conversations)
        {
            _conversations = conversations;
        }

        public static ConversationIndex Read(string path)
        {
            var conversations = new Dictionary<int, ConversationRecord>();
            foreach (string line in File.ReadLines(path))
            {
                if (string.IsNullOrWhiteSpace(line))
                {
                    continue;
                }

                ConversationRecord? conversation = JsonSerializer.Deserialize<ConversationRecord>(line,
                    new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
                if (conversation == null)
                {
                    throw new InvalidDataException("The conversation index contains a null record.");
                }

                conversations.Add(conversation.Id, conversation);
            }

            return new ConversationIndex(conversations);
        }

        public LookAheadGraph BuildGraph(int conversationId)
        {
            var pending = new Queue<int>();
            var group = new HashSet<int>();
            if (!_conversations.ContainsKey(conversationId))
            {
                throw new ArgumentException($"Conversation {conversationId} is not in the index.");
            }

            group.Add(conversationId);
            pending.Enqueue(conversationId);
            while (pending.Count > 0)
            {
                ConversationRecord conversation = _conversations[pending.Dequeue()];
                foreach (EntryRecord entry in conversation.Entries)
                {
                    foreach (int destination in entry.ToConversation)
                    {
                        if (_conversations.ContainsKey(destination) && group.Add(destination))
                        {
                            pending.Enqueue(destination);
                        }
                    }
                }
            }

            var symbols = new StateSymbols();
            var nodes = new List<LookAheadNode>();
            foreach (int id in group)
            {
                foreach (EntryRecord entry in _conversations[id].Entries)
                {
                    nodes.Add(ToNode(id, entry, symbols));
                }
            }

            return new LookAheadGraph(nodes, symbols);
        }

        public IReadOnlyList<DialogueNodeId> EntriesIn(int conversationId)
        {
            if (!_conversations.TryGetValue(conversationId, out ConversationRecord? conversation))
            {
                throw new ArgumentException($"Conversation {conversationId} is not in the index.");
            }

            var ids = new List<DialogueNodeId>(conversation.Entries.Count);
            foreach (EntryRecord entry in conversation.Entries)
            {
                ids.Add(new DialogueNodeId(conversationId, entry.Id));
            }

            return ids;
        }

        private static LookAheadNode ToNode(int conversationId, EntryRecord entry, StateSymbols symbols)
        {
            GuardParser.TryParse(entry.Guard, out GuardExpression guard);
            DialogueCheckKind kind = KindOf(entry.Fields);
            int flagSlot = -1;
            int failedFlagSlot = -1;
            if ((kind == DialogueCheckKind.Red || kind == DialogueCheckKind.White)
                && entry.Fields.TryGetValue(FlagNameField, out string? flag)
                && !string.IsNullOrWhiteSpace(flag))
            {
                flagSlot = symbols.Variable(flag);
                failedFlagSlot = symbols.Variable(flag + "_failed");
            }

            int cost = ReadInt(entry.Fields, ClickCostField);
            bool booleanOnly = kind == DialogueCheckKind.KimSwitch
                && ReadBoolean(entry.Fields, BooleanOnlyField);
            bool closesOnceSeen = kind == DialogueCheckKind.Fake
                || (kind == DialogueCheckKind.KimSwitch && !booleanOnly);
            var links = new List<DialogueNodeId>(entry.To.Count);
            for (int i = 0; i < entry.To.Count; i++)
            {
                int destinationConversation = i < entry.ToConversation.Count
                    ? entry.ToConversation[i]
                    : conversationId;
                links.Add(new DialogueNodeId(destinationConversation, entry.To[i]));
            }

            return new LookAheadNode(
                new DialogueNodeId(conversationId, entry.Id), entry.Group, kind, guard,
                ActionParser.Parse(entry.Script, symbols), links, Math.Max(cost, 0),
                ReadBoolean(entry.Fields, CostOnceField),
                ReadBoolean(entry.Fields, HiddenNotEnoughField), flagSlot, failedFlagSlot,
                booleanOnly,
                closesOnceSeen ? symbols.Seen(new DialogueNodeId(conversationId, entry.Id)) : -1);
        }

        private static DialogueCheckKind KindOf(IReadOnlyDictionary<string, string> fields)
        {
            if (fields.ContainsKey(PassiveField)) return DialogueCheckKind.Passive;
            if (fields.ContainsKey(RedField)) return DialogueCheckKind.Red;
            if (fields.ContainsKey(WhiteField)) return DialogueCheckKind.White;
            if (fields.ContainsKey(FakeField)) return DialogueCheckKind.Fake;
            if (fields.ContainsKey(TestField)) return DialogueCheckKind.Test;
            if (fields.ContainsKey(KimWatchField)) return DialogueCheckKind.KimSwitch;
            return DialogueCheckKind.None;
        }

        private static bool ReadBoolean(IReadOnlyDictionary<string, string> fields, string name)
        {
            return fields.TryGetValue(name, out string? value)
                && bool.TryParse(value, out bool parsed) && parsed;
        }

        private static int ReadInt(IReadOnlyDictionary<string, string> fields, string name)
        {
            return fields.TryGetValue(name, out string? value)
                && int.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out int parsed)
                ? parsed : 0;
        }
    }

    internal sealed class ConversationRecord
    {
        public int Id { get; set; }
        public List<EntryRecord> Entries { get; set; } = new List<EntryRecord>();
    }

    internal sealed class EntryRecord
    {
        public int Id { get; set; }
        public bool Group { get; set; }
        public string Guard { get; set; } = string.Empty;
        public string Script { get; set; } = string.Empty;
        public List<int> To { get; set; } = new List<int>();
        [JsonPropertyName("to_conversation")]
        public List<int> ToConversation { get; set; } = new List<int>();
        public Dictionary<string, string> Fields { get; set; } = new Dictionary<string, string>();
    }
}
