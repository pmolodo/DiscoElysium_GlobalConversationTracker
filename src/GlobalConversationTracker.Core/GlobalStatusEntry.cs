using System;

namespace GlobalConversationTracker
{
    /// <summary>
    /// A single (conversation, dialogue entry, status) triple: one flattened row of
    /// <see cref="GlobalConversationState"/>.
    /// </summary>
    public readonly struct GlobalStatusEntry : IEquatable<GlobalStatusEntry>
    {
        /// <summary>Creates an entry.</summary>
        public GlobalStatusEntry(int conversationId, int dialogueEntryId, SimStatus status)
        {
            ConversationId = conversationId;
            DialogueEntryId = dialogueEntryId;
            Status = status;
        }

        /// <summary>The conversation's integer ID.</summary>
        public int ConversationId { get; }

        /// <summary>The dialogue entry's integer ID, unique within its conversation.</summary>
        public int DialogueEntryId { get; }

        /// <summary>The status recorded for this entry.</summary>
        public SimStatus Status { get; }

        /// <summary>Deconstructs into its three components.</summary>
        public void Deconstruct(out int conversationId, out int dialogueEntryId, out SimStatus status)
        {
            conversationId = ConversationId;
            dialogueEntryId = DialogueEntryId;
            status = Status;
        }

        /// <inheritdoc />
        public bool Equals(GlobalStatusEntry other)
        {
            return ConversationId == other.ConversationId
                && DialogueEntryId == other.DialogueEntryId
                && Status == other.Status;
        }

        /// <inheritdoc />
        public override bool Equals(object? obj) => obj is GlobalStatusEntry other && Equals(other);

        /// <inheritdoc />
        public override int GetHashCode()
        {
            unchecked
            {
                int hash = 17;
                hash = (hash * 31) + ConversationId;
                hash = (hash * 31) + DialogueEntryId;
                hash = (hash * 31) + (int)Status;
                return hash;
            }
        }

        /// <summary>Equality operator.</summary>
        public static bool operator ==(GlobalStatusEntry left, GlobalStatusEntry right) => left.Equals(right);

        /// <summary>Inequality operator.</summary>
        public static bool operator !=(GlobalStatusEntry left, GlobalStatusEntry right) => !left.Equals(right);

        /// <inheritdoc />
        public override string ToString()
        {
            return $"GlobalStatusEntry({ConversationId}, {DialogueEntryId}, SimStatus.{Status})";
        }
    }
}
