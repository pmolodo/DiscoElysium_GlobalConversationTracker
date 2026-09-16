// SPDX-License-Identifier: MIT
namespace GlobalConversationTracker.Engine
{
    /// <summary>
    /// The entry fields the look-ahead engine reads, spelled as the database spells them.
    /// </summary>
    /// <remarks>
    /// <para>The one C# copy of the list. The extractor trims a shipped index down to
    /// these, the plugin canonicalises the live database over these, and both have to mean
    /// the same thing by "the content of a conversation" or a cache check compares two
    /// different questions.</para>
    ///
    /// <para>It must also match <c>lookahead_engine::index::ENTRY_FIELDS_READ</c>, which it
    /// cannot share a constant with across the language boundary - so
    /// <c>tests/shipped_index.rs</c> checks the two against each other. That check is not
    /// ceremony: the first draft of the trim guessed these names, got all eleven wrong, and
    /// would have produced an index in which no entry was a skill check, with nothing
    /// failing to say so.</para>
    /// </remarks>
    public static class IndexFields
    {
        /// <summary>
        /// The fields, in the same order as the Rust constant so the two can be read side
        /// by side. Alphabetical would be tidier and would make the comparison harder.
        /// </summary>
        public static readonly string[] Read =
        {
            "Actor",
            "DifficultyPass",
            "Antipassive",
            "DifficultyRed",
            "DifficultyWhite",
            "DifficultyAtmo",
            "AlwaysSucceed",
            "HiddenTest",
            "kim_watch",
            "boolean_only",
            "FlagName",
            "SkillType",
            "ClickCost",
            "CostOnce",
            "HiddenNotEnough",
        };

        /// <summary>
        /// The CONVERSATION fields the engine reads: a journal task's condition variables.
        /// </summary>
        /// <remarks>
        /// <para>What <c>JournalImporter.Populate</c> reads to build the journal - a task's
        /// show, done and cancel conditions, and the same for each of up to twelve subtasks.
        /// The engine resolves a <c>GainTask</c>, <c>FinishTask</c> or <c>CancelTask</c>
        /// argument through them to the task or subtask it names, since any of a part's
        /// three variables names it.</para>
        ///
        /// <para>Must match <c>lookahead_engine::index::CONVERSATION_FIELDS_READ</c>, checked
        /// by <c>tests/shipped_index.rs</c> like the entry list.</para>
        /// </remarks>
        public static readonly string[] ConversationRead = JournalFields();

        /// <summary>The main task's three conditions, then each subtask's three in order.</summary>
        private static string[] JournalFields()
        {
            var names = new System.Collections.Generic.List<string>
            {
                "display_condition_main",
                "done_condition_main",
                "cancel_condition_main",
            };
            for (int subtask = 1; subtask <= JournalSubtaskLimit; subtask++)
            {
                string number = subtask.ToString("00", System.Globalization.CultureInfo.InvariantCulture);
                names.Add("display_subtask_" + number);
                names.Add("done_subtask_" + number);
                names.Add("cancel_subtask_" + number);
            }

            return names.ToArray();
        }

        /// <summary>How many subtasks a task can have: <c>JournalImporter.MAX_NR_OF_SUBTASKS</c>.</summary>
        public const int JournalSubtaskLimit = 12;
    }
}
