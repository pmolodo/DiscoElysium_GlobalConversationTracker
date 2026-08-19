namespace UnifiedConversationTracker.Core
{
    /// <summary>
    /// One SimStatus reading taken out of the running game: which conversation,
    /// which dialogue entry, and the game's own status string.
    /// </summary>
    /// <remarks>
    /// The status is carried as the raw string rather than a <see cref="SimStatus"/>
    /// on purpose. The game's value is a Lua string, and an unrecognized one has to
    /// be skippable and countable exactly the way the file loader skips a damaged
    /// row, instead of throwing part way through a walk and losing the rest.
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
        /// The game's status string, expected to be one of Untouched / WasOffered /
        /// WasDisplayed. Anything else is skipped and counted by the caller.
        /// </summary>
        public string? StatusName { get; }

        /// <inheritdoc />
        public override string ToString() =>
            $"SimStatusRow({ConversationId}, {DialogueEntryId}, {StatusName ?? "null"})";
    }
}
