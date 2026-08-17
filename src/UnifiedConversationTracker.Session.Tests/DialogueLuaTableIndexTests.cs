using Xunit;

namespace UnifiedConversationTracker.Session.Tests
{
    /// <summary>
    /// The replica of the game's <c>DialogueLua.StringToTableIndex</c>. Every case
    /// here is read straight off the decompiled original: an id this rewrites
    /// differently from the game is a conversation whose compressed blob is never
    /// found.
    /// </summary>
    public sealed class DialogueLuaTableIndexTests
    {
        [Theory]
        [InlineData("0x0100000400000281", "0x0100000400000281")]
        [InlineData("zq36uckabuyqrblk", "zq36uckabuyqrblk")]
        public void Of_LeavesAnOrdinaryArticyIdAlone(string value, string expected)
        {
            Assert.Equal(expected, DialogueLuaTableIndex.Of(value));
        }

        [Theory]
        [InlineData("-tmmrhn0fyuu915p", "_tmmrhn0fyuu915p")]
        [InlineData("a-b-c", "a_b_c")]
        [InlineData("a b", "a_b")]
        [InlineData("a(b)", "a_b_")]
        [InlineData("a\"b", "a_b")]
        public void Of_ReplacesEveryCharacterTheGameReplaces(string value, string expected)
        {
            Assert.Equal(expected, DialogueLuaTableIndex.Of(value));
        }

        [Fact]
        public void Of_DropsCarriageReturnsAndEscapesNewlines()
        {
            // DoubleQuotesToSingle's other two substitutions, which survive the leading
            // quote replacement that makes its first one dead.
            Assert.Equal("a\\nb", DialogueLuaTableIndex.Of("a\nb"));
            Assert.Equal("ab", DialogueLuaTableIndex.Of("a\rb"));
        }

        [Theory]
        [InlineData(null)]
        [InlineData("")]
        public void Of_ForNothing_ReturnsTheEmptyStringLikeTheGameDoes(string? value)
        {
            Assert.Equal(string.Empty, DialogueLuaTableIndex.Of(value));
        }
    }
}
