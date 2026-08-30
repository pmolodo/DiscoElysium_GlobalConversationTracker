// SPDX-License-Identifier: MIT
using System;
using System.Collections.Generic;
using System.Text;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>
    /// Everything the crawl carries along a path: one int per slot, plus money.
    /// </summary>
    /// <remarks>
    /// <para>Immutable, with a hash computed once at construction, because the search's
    /// visited set is keyed on (node, state) and probes it far more often than it builds
    /// one.</para>
    ///
    /// <para>Money is a separate field rather than another slot because it is the one
    /// quantity compared against arbitrary constants rather than toggled, and because it
    /// is the one the termination argument rests on: <c>HaveMoney</c> gates every
    /// purchase, so a balance can only fall, and it can never fall below zero. That
    /// bounds how often a path can traverse a purchase, which is what stops the search
    /// looping in a graph where 72% of nodes sit inside a cycle.</para>
    /// </remarks>
    public sealed class LookAheadState : IEquatable<LookAheadState>
    {
        private readonly int[] _slots;
        private readonly int _hash;

        private LookAheadState(int[] slots, int money, int dayMinutes)
        {
            _slots = slots;
            Money = money;
            DayMinutes = dayMinutes;
            unchecked
            {
                int hash = (money * 486187739) ^ (dayMinutes * 43112609);
                for (int i = 0; i < slots.Length; i++)
                {
                    if (slots[i] != 0)
                    {
                        hash = (hash * 31) ^ (i * 397) ^ slots[i];
                    }
                }

                _hash = hash;
            }
        }

        /// <summary>An all-zero state with the given balance.</summary>
        /// <param name="slotCount">How many slots the symbol table has.</param>
        /// <param name="money">The starting balance, in centimes.</param>
        /// <param name="dayMinutes">The clock, in minutes since midnight.</param>
        /// <exception cref="ArgumentOutOfRangeException">A count or balance is negative.</exception>
        public static LookAheadState Empty(int slotCount, int money, int dayMinutes = 0)
        {
            if (slotCount < 0)
            {
                throw new ArgumentOutOfRangeException(nameof(slotCount));
            }

            if (money < 0)
            {
                throw new ArgumentOutOfRangeException(
                    nameof(money), "The player's balance cannot be negative.");
            }

            return new LookAheadState(new int[slotCount], money, dayMinutes);
        }

        /// <summary>The player's balance, in centimes.</summary>
        public int Money { get; }

        /// <summary>
        /// The clock, in minutes since midnight. Advanced only by <c>PassTime()</c>, and
        /// only ever within the day - the story's day counter is not something a
        /// conversation can move.
        /// </summary>
        public int DayMinutes { get; }

        /// <summary>How many slots this state holds.</summary>
        public int SlotCount => _slots.Length;

        /// <summary>The value in a slot.</summary>
        /// <param name="index">The slot index.</param>
        public int Get(int index)
        {
            return (uint)index < (uint)_slots.Length ? _slots[index] : 0;
        }

        /// <summary>Whether a slot holds anything other than zero.</summary>
        /// <param name="index">The slot index.</param>
        public bool IsSet(int index)
        {
            return Get(index) != 0;
        }

        /// <summary>This state with one slot changed.</summary>
        /// <param name="index">The slot index.</param>
        /// <param name="value">Its new value.</param>
        public LookAheadState With(int index, int value)
        {
            if (Get(index) == value)
            {
                return this;
            }

            int[] slots = Resize(index);
            slots[index] = value;
            return new LookAheadState(slots, Money, DayMinutes);
        }

        /// <summary>This state with the balance changed.</summary>
        /// <param name="money">The new balance, clamped at zero.</param>
        public LookAheadState WithMoney(int money)
        {
            int clamped = money < 0 ? 0 : money;
            return clamped == Money ? this : new LookAheadState(Copy(), clamped, DayMinutes);
        }

        /// <summary>This state with the clock moved.</summary>
        /// <param name="dayMinutes">The new minute count, wrapped into the day.</param>
        public LookAheadState WithDayMinutes(int dayMinutes)
        {
            int wrapped = ((dayMinutes % ClockTime.MinutesInADay) + ClockTime.MinutesInADay)
                % ClockTime.MinutesInADay;
            return wrapped == DayMinutes
                ? this
                : new LookAheadState(Copy(), Money, wrapped);
        }

        /// <summary>This state with several slots changed at once.</summary>
        /// <param name="changes">Slot index to new value.</param>
        /// <param name="money">The new balance, clamped at zero.</param>
        /// <param name="dayMinutes">The new minute count.</param>
        public LookAheadState With(
            IReadOnlyList<KeyValuePair<int, int>> changes, int money, int dayMinutes)
        {
            if (changes == null)
            {
                throw new ArgumentNullException(nameof(changes));
            }

            int highest = _slots.Length - 1;
            for (int i = 0; i < changes.Count; i++)
            {
                if (changes[i].Key > highest)
                {
                    highest = changes[i].Key;
                }
            }

            int[] slots = Resize(highest);
            for (int i = 0; i < changes.Count; i++)
            {
                slots[changes[i].Key] = changes[i].Value;
            }

            int wrapped = ((dayMinutes % ClockTime.MinutesInADay) + ClockTime.MinutesInADay)
                % ClockTime.MinutesInADay;
            return new LookAheadState(slots, money < 0 ? 0 : money, wrapped);
        }

        private int[] Copy()
        {
            var slots = new int[_slots.Length];
            Array.Copy(_slots, slots, _slots.Length);
            return slots;
        }

        /// <summary>
        /// A copy at least large enough to hold <paramref name="index"/>. Slots are
        /// interned lazily as the graph is read, so a state built early can be narrower
        /// than one built later; growing here keeps that from being the caller's problem.
        /// </summary>
        private int[] Resize(int index)
        {
            int length = Math.Max(_slots.Length, index + 1);
            var slots = new int[length];
            Array.Copy(_slots, slots, _slots.Length);
            return slots;
        }

        /// <inheritdoc/>
        public bool Equals(LookAheadState? other)
        {
            if (ReferenceEquals(this, other))
            {
                return true;
            }

            if (other is null || other._hash != _hash || other.Money != Money
                || other.DayMinutes != DayMinutes)
            {
                return false;
            }

            int shared = Math.Min(_slots.Length, other._slots.Length);
            for (int i = 0; i < shared; i++)
            {
                if (_slots[i] != other._slots[i])
                {
                    return false;
                }
            }

            // Slots past the shorter state's end are zero by definition, so the longer
            // one only matches if its tail is empty.
            return TailIsEmpty(_slots, shared) && TailIsEmpty(other._slots, shared);
        }

        private static bool TailIsEmpty(int[] slots, int from)
        {
            for (int i = from; i < slots.Length; i++)
            {
                if (slots[i] != 0)
                {
                    return false;
                }
            }

            return true;
        }

        /// <inheritdoc/>
        public override bool Equals(object? obj)
        {
            return Equals(obj as LookAheadState);
        }

        /// <inheritdoc/>
        public override int GetHashCode()
        {
            return _hash;
        }

        /// <summary>The set slots and the balance, for test failures worth reading.</summary>
        /// <param name="symbols">The table the slot indices belong to.</param>
        public string Describe(StateSymbols symbols)
        {
            if (symbols == null)
            {
                throw new ArgumentNullException(nameof(symbols));
            }

            var builder = new StringBuilder();
            builder.Append("money=").Append(Money)
                .Append(", clock=").Append(ClockTime.HoursOf(DayMinutes))
                .Append(':').Append((DayMinutes % 60).ToString("00"));
            for (int i = 0; i < _slots.Length; i++)
            {
                if (_slots[i] != 0)
                {
                    builder.Append(", ").Append(symbols.NameOf(i)).Append('=').Append(_slots[i]);
                }
            }

            return builder.ToString();
        }

        /// <inheritdoc/>
        public override string ToString()
        {
            return "money=" + Money + ", dayMinutes=" + DayMinutes
                + ", slots=" + _slots.Length;
        }
    }
}
