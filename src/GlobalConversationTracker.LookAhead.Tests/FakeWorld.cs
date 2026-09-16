// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using GlobalConversationTracker.Engine;

namespace GlobalConversationTracker.LookAhead.Tests
{
    /// <summary>A player situation a test can dictate outright.</summary>
    internal sealed class FakeWorld : ILookAheadWorld
    {
        private readonly Dictionary<string, GuardValue> _variables =
            new Dictionary<string, GuardValue>();
        private readonly HashSet<string> _items = new HashSet<string>();
        private readonly Dictionary<string, GuardValue> _queries =
            new Dictionary<string, GuardValue>();
        private readonly Dictionary<DialogueNodeId, Ternary> _checks =
            new Dictionary<DialogueNodeId, Ternary>();
        private readonly HashSet<DialogueNodeId> _seen = new HashSet<DialogueNodeId>();

        public int Money { get; set; }

        public int DayMinutes { get; set; }

        public int DayCounter { get; set; } = 1;

        public bool IsClockLocked { get; set; }

        /// <summary>What an unlisted check does. Unknown makes the engine try both.</summary>
        public Ternary DefaultCheck { get; set; } = Ternary.Unknown;

        public FakeWorld WithMoney(int centimes)
        {
            Money = centimes;
            return this;
        }

        /// <summary>Sets the clock, as an hour and minute of the day.</summary>
        public FakeWorld AtTime(int hour, int minute = 0)
        {
            DayMinutes = (hour * 60) + minute;
            return this;
        }

        public FakeWorld WithDay(int day)
        {
            DayCounter = day;
            return this;
        }

        public FakeWorld WithLockedClock()
        {
            IsClockLocked = true;
            return this;
        }

        public FakeWorld WithVariable(string name, bool value)
        {
            _variables[name] = GuardValue.FromBoolean(value);
            return this;
        }

        public FakeWorld WithVariable(string name, double value)
        {
            _variables[name] = GuardValue.FromNumber(value);
            return this;
        }

        public FakeWorld WithItem(string name)
        {
            _items.Add(name);
            return this;
        }

        public FakeWorld WithQuery(string name, bool value)
        {
            _queries[name] = GuardValue.FromBoolean(value);
            return this;
        }

        public FakeWorld WithQuery(string name, double value)
        {
            _queries[name] = GuardValue.FromNumber(value);
            return this;
        }

        public FakeWorld WithCheck(DialogueNodeId node, Ternary outcome)
        {
            _checks[node] = outcome;
            return this;
        }

        public GuardValue GetVariable(string name)
        {
            return _variables.TryGetValue(name, out GuardValue value)
                ? value
                : GuardValue.FromBoolean(false);
        }

        public bool HasItem(string name)
        {
            return _items.Contains(name);
        }

        public GuardValue Query(string name, IReadOnlyList<GuardValue> arguments)
        {
            // Keyed on name alone: no test here needs two different answers for the
            // same function with different arguments.
            return _queries.TryGetValue(name, out GuardValue value)
                ? value
                : GuardValue.Unknown;
        }

        public FakeWorld WithSeen(DialogueNodeId node)
        {
            _seen.Add(node);
            return this;
        }

        public bool IsSeen(DialogueNodeId node)
        {
            return _seen.Contains(node);
        }

        public Ternary CheckPasses(DialogueNodeId node)
        {
            return _checks.TryGetValue(node, out Ternary outcome) ? outcome : DefaultCheck;
        }
    }
}
