// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Text.Json;
using GlobalConversationTracker.LookAhead;

namespace GlobalConversationTracker.LookAheadOffline
{
    internal sealed class OfflineState
    {
        public int Money { get; set; }
        public int DayMinutes { get; set; }
        public int DayCounter { get; set; } = 1;
        public bool ClockLocked { get; set; }
        public Dictionary<string, JsonElement> Variables { get; set; } = new Dictionary<string, JsonElement>();
        public HashSet<string> Items { get; set; } = new HashSet<string>();
        public HashSet<string> Tasks { get; set; } = new HashSet<string>();
        public HashSet<string> LocalSeen { get; set; } = new HashSet<string>();
        public HashSet<string> GlobalSeen { get; set; } = new HashSet<string>();
        public Dictionary<string, JsonElement> Queries { get; set; } = new Dictionary<string, JsonElement>();
        public Dictionary<string, int> CounterCaps { get; set; } = new Dictionary<string, int>();
    }

    internal sealed class OfflineWorld : ILookAheadWorld
    {
        private readonly OfflineState _state;

        public OfflineWorld(OfflineState state)
        {
            _state = state;
        }

        public int Money => _state.Money;
        public int DayMinutes => _state.DayMinutes;
        public int DayCounter => _state.DayCounter;
        public bool IsClockLocked => _state.ClockLocked;

        public GuardValue GetVariable(string name)
        {
            return _state.Variables.TryGetValue(name, out JsonElement value)
                ? Convert(value) : GuardValue.Unknown;
        }

        public bool HasItem(string name) => _state.Items.Contains(name);
        public bool IsTaskActive(string name) => _state.Tasks.Contains(name);

        public GuardValue Query(string name, IReadOnlyList<GuardValue> arguments)
        {
            return _state.Queries.TryGetValue(name, out JsonElement value)
                ? Convert(value) : GuardValue.Unknown;
        }

        public Ternary CheckPasses(DialogueNodeId node) => Ternary.Unknown;

        public bool IsSeen(DialogueNodeId node) => _state.LocalSeen.Contains(Key(node));

        public Novelty GetNovelty(DialogueNodeId node)
        {
            string key = Key(node);
            return _state.LocalSeen.Contains(key) ? Novelty.SeenThisGame
                : _state.GlobalSeen.Contains(key) ? Novelty.UnseenThisGame
                : Novelty.UnseenAnyGame;
        }

        private static string Key(DialogueNodeId node) => node.ConversationId + ":" + node.EntryId;

        private static GuardValue Convert(JsonElement value)
        {
            return value.ValueKind switch
            {
                JsonValueKind.True => GuardValue.FromBoolean(true),
                JsonValueKind.False => GuardValue.FromBoolean(false),
                JsonValueKind.Number when value.TryGetDouble(out double number) => GuardValue.FromNumber(number),
                JsonValueKind.String => GuardValue.FromText(value.GetString() ?? string.Empty),
                _ => GuardValue.Unknown,
            };
        }
    }
}
