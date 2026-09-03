// SPDX-License-Identifier: MIT
using Xunit;

namespace GlobalConversationTracker.DialogueAsset.Tests
{
    /// <summary>How a scalar written by Unity reads back.</summary>
    public class DialogueScalarTests
    {
        [Theory]
        [InlineData("START", "START")]
        [InlineData("  START  ", "START")]
        [InlineData("", "")]
        [InlineData("'quoted'", "quoted")]
        [InlineData("'It''s'", "It's")]
        [InlineData("''", "")]
        [InlineData("'", "'")]
        [InlineData("\"plain\"", "plain")]
        [InlineData("\"say \\\"hi\\\"\"", "say \"hi\"")]
        [InlineData("\"one \\\\ two\"", "one \\ two")]
        // Not a general YAML unescape: the index has never turned these into breaks, and
        // the guard and script corpus is where a reader that does belongs.
        [InlineData("\"line\\nbreak\"", "line\\nbreak")]
        public void DecodesWhatUnityWrote(string written, string expected)
        {
            Assert.Equal(expected, DialogueScalar.Decode(written));
        }

        [Theory]
        [InlineData("12", 12)]
        [InlineData(" 12 ", 12)]
        [InlineData("-3", -3)]
        [InlineData("", null)]
        [InlineData("not-a-number", null)]
        [InlineData("0x0000000000000001", null)]
        public void ReadsANumberOrNothing(string written, int? expected)
        {
            Assert.Equal(expected, DialogueScalar.AsInt(written));
        }

        [Fact]
        public void RefusesAnUnreadableId()
        {
            // An id that will not parse is a broken asset, not a record to guess at.
            Assert.Throws<FormatException>(() => DialogueScalar.ParseId("later"));
        }
    }
}
