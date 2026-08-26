using System;

namespace GlobalConversationTracker.CrashHarness
{
    /// <summary>
    /// The two states the crash harness knows how to write. Lives here rather than in
    /// the test project so the process being killed and the test inspecting the
    /// wreckage agree on what "the old generation" and "the new generation" contain.
    /// </summary>
    public static class CrashHarnessPayloads
    {
        /// <summary>Name of the small "previous generation" payload.</summary>
        public const string OldName = "old";

        /// <summary>Name of the large "new generation" payload.</summary>
        public const string NewName = "new";

        /// <summary>Conversation ID present only in the new payload.</summary>
        public const int NewOnlyConversationId = 9001;

        /// <summary>Dialogue entry ID present only in the new payload.</summary>
        public const int NewOnlyDialogueEntryId = 7;

        private const int NewConversationCount = 40;
        private const int NewEntriesPerConversation = 25;

        /// <summary>Builds one of the named payloads.</summary>
        /// <exception cref="ArgumentException">The name is not a known payload.</exception>
        public static GlobalConversationState Build(string name)
        {
            var state = new GlobalConversationState();
            switch (name)
            {
                case OldName:
                    state.Merge(1, 10, SimStatus.WasDisplayed);
                    state.Merge(1, 11, SimStatus.WasOffered);
                    state.Merge(2, 20, SimStatus.WasOffered);
                    return state;

                case NewName:
                    // Deliberately far larger than the old payload, so the temp file takes
                    // several write chunks and a kill part way through leaves a genuinely
                    // truncated file.
                    for (int conversationId = 1; conversationId <= NewConversationCount; conversationId++)
                    {
                        for (int entryId = 1; entryId <= NewEntriesPerConversation; entryId++)
                        {
                            state.Merge(conversationId, entryId, SimStatus.WasDisplayed);
                        }
                    }

                    state.Merge(NewOnlyConversationId, NewOnlyDialogueEntryId, SimStatus.WasOffered);
                    return state;

                default:
                    throw new ArgumentException($"Unknown payload '{name}'.", nameof(name));
            }
        }
    }
}
