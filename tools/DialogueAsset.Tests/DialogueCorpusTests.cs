// SPDX-License-Identifier: MIT
using Xunit;

namespace GlobalConversationTracker.DialogueAsset.Tests
{
    /// <summary>What the guard and script corpus reads out of a database, and how it writes it.</summary>
    public class DialogueCorpusTests
    {
        private static readonly string FixtureAsset =
            Path.Combine(AppContext.BaseDirectory, "Fixtures", "mini-database.asset");

        /// <summary>
        /// A database written inline rather than beside the mini fixture, because the
        /// corpus turns on escapes the conversation index deliberately does not decode,
        /// and the fixture's expected index is a record of that other reading.
        /// </summary>
        private const string Database = """
              conversations:
              - id: 1
                dialogueEntries:
                - id: 0
                  conditionsString: "line\nbreak"
                  userScript: "tab\there"
                - id: 1
                  conditionsString: "line\nbreak"
                  userScript: ""
                - id: 2
                  conditionsString: Variable["a"] == true
                  userScript: 'It''s spent'
              syncInfo:
                syncActors: 0
            """;

        [Theory]
        // The three the conversation index leaves as written, and the whole reason this
        // decoder exists rather than a widened DialogueScalar.
        [InlineData("\"line\\nbreak\"", "line\nbreak")]
        [InlineData("\"tab\\there\"", "tab\there")]
        [InlineData("\"back\\rup\"", "back\rup")]
        // An unknown escape is the character it precedes, so an escaped quote or an
        // escaped backslash reads the same as it does in the index.
        [InlineData("\"say \\\"hi\\\"\"", "say \"hi\"")]
        [InlineData("\"one \\\\ two\"", "one \\ two")]
        [InlineData("'It''s'", "It's")]
        [InlineData("Variable[\"a\"] == true", "Variable[\"a\"] == true")]
        // Nothing is trimmed: what is recorded is the scalar as it was written.
        [InlineData("  padded  ", "  padded  ")]
        public void DecodesEveryBreakTheIndexLeavesAlone(string written, string expected)
        {
            Assert.Equal(expected, DialogueCorpusExtractor.Decode(written));
        }

        [Fact]
        public void KeepsEachDistinctNonBlankRecordOnce()
        {
            DialogueCorpus corpus = DialogueCorpusExtractor.Extract(new StringReader(Database));

            // The repeated guard once, the empty script not at all, and ordinal order -
            // which is over the decoded record, not over the escaped line written for it.
            Assert.Equal(new[] { "Variable[\"a\"] == true", "line\nbreak" }, corpus.Guards);
            Assert.Equal(new[] { "It's spent", "tab\there" }, corpus.Scripts);
        }

        [Fact]
        public void ReadsOnlyTheConversationsSection()
        {
            DialogueCorpus corpus = DialogueCorpusExtractor.Extract(FixtureAsset);

            // The fixture's actor above the section and its conversation below syncInfo are
            // both shaped like entries, and neither is one the corpus may see.
            Assert.Equal(new[] { "Variable[\"kim.trust\"] > 2 and Variable[\"money\"] == true" }, corpus.Guards);
            Assert.Equal(new[] { "Money = Money - 50" }, corpus.Scripts);
        }

        [Fact]
        public void WritesOneRecordPerLineWithTheBreaksEscaped()
        {
            var writer = new StringWriter { NewLine = "\n" };

            DialogueCorpusFile.Write(writer, new[] { "a\r\nb", "back \\ slash", "already \\n" });

            // The backslash goes first, so "already \n" comes back as itself and not as a
            // break the reader would undo.
            Assert.Equal("a\\r\\nb\nback \\\\ slash\nalready \\\\n\n", writer.ToString());
        }
    }
}
