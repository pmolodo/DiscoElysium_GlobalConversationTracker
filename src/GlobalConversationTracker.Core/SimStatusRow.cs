// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Core
{
    /// <summary>
    /// One SimStatus reading taken out of the running game: which conversation,
    /// which dialogue entry, and the game's own status string.
    /// </summary>
    /// <remarks>
    /// The status stays a raw string rather than a <see cref="SimStatus"/> so an
    /// unrecognized value can be skipped and counted the way the file loader skips
    /// a damaged row, instead of throwing part way through a parse and losing every
    /// row after it.
    /// </remarks>
    public readonly struct SimStatusRow
    {
        /// <summary>Creates a row.</summary>
        public SimStatusRow(int conversationId, int dialogueEntryId, string? statusName)
        {
            ConversationId = conversationId;
            DialogueEntryId = dialogueEntryId;
            StatusName = statusName;
        }

        /// <summary>The conversation's integer ID.</summary>
        public int ConversationId { get; }

        /// <summary>The dialogue entry's integer ID.</summary>
        public int DialogueEntryId { get; }

        /// <summary>
        /// The game's status string: Untouched / WasOffered / WasDisplayed. Anything
        /// else is skipped and counted by the caller.
        /// </summary>
        public string? StatusName { get; }

        /// <inheritdoc />
        public override string ToString() =>
            $"SimStatusRow({ConversationId}, {DialogueEntryId}, {StatusName ?? "null"})";
    }
}
