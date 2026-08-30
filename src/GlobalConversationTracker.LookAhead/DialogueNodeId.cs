// SPDX-License-Identifier: MIT
using System;

namespace GlobalConversationTracker.LookAhead
{
    /// <summary>
    /// Identifies one dialogue entry: the conversation it belongs to, and its id
    /// within that conversation.
    /// </summary>
    /// <remarks>
    /// <para>Both halves are needed. Entry ids restart at 0 in every conversation, and
    /// links cross conversation boundaries - 680 of them in the shipped database - so
    /// an entry id alone is ambiguous the moment a crawl leaves its starting
    /// conversation.</para>
    ///
    /// <para>A struct because the search puts millions of these in dictionary keys
    /// alongside a state vector; a class would make every visited-set probe an
    /// allocation and a pointer chase.</para>
    /// </remarks>
    public readonly struct DialogueNodeId : IEquatable<DialogueNodeId>
    {
        /// <summary>Creates an id.</summary>
        /// <param name="conversationId">The conversation's integer ID.</param>
        /// <param name="entryId">The entry's integer ID within that conversation.</param>
        public DialogueNodeId(int conversationId, int entryId)
        {
            ConversationId = conversationId;
            EntryId = entryId;
        }

        /// <summary>The conversation's integer ID.</summary>
        public int ConversationId { get; }

        /// <summary>The entry's integer ID within its conversation.</summary>
        public int EntryId { get; }

        /// <inheritdoc/>
        public bool Equals(DialogueNodeId other)
        {
            return ConversationId == other.ConversationId && EntryId == other.EntryId;
        }

        /// <inheritdoc/>
        public override bool Equals(object? obj)
        {
            return obj is DialogueNodeId other && Equals(other);
        }

        /// <inheritdoc/>
        public override int GetHashCode()
        {
            unchecked
            {
                return (ConversationId * 397) ^ EntryId;
            }
        }

        /// <summary>Parseable form: <c>new DialogueNodeId(825, 189)</c>.</summary>
        public override string ToString()
        {
            return $"new DialogueNodeId({ConversationId}, {EntryId})";
        }

        /// <summary>Value equality.</summary>
        /// <param name="left">The left operand.</param>
        /// <param name="right">The right operand.</param>
        public static bool operator ==(DialogueNodeId left, DialogueNodeId right)
        {
            return left.Equals(right);
        }

        /// <summary>Value inequality.</summary>
        /// <param name="left">The left operand.</param>
        /// <param name="right">The right operand.</param>
        public static bool operator !=(DialogueNodeId left, DialogueNodeId right)
        {
            return !left.Equals(right);
        }
    }
}
