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
            "DifficultyPass",
            "DifficultyRed",
            "DifficultyWhite",
            "DifficultyAtmo",
            "HiddenTest",
            "kim_watch",
            "boolean_only",
            "FlagName",
            "ClickCost",
            "CostOnce",
            "HiddenNotEnough",
        };
    }
}
